use std::{net::{IpAddr, SocketAddr}, io::Cursor, borrow::BorrowMut, sync::Arc, time::Duration};

use anyhow::anyhow;
use bytes::{Buf, BytesMut, BufMut};
use crypto::{sha1::Sha1, digest::Digest};
use tokio::{net::{TcpStream}, io::{AsyncWriteExt, AsyncReadExt}, sync::{mpsc::{Receiver, Sender}, Mutex}};

use crate::{bitfield::BitField};

#[derive(Debug, Clone)]
pub enum PeerMsg {
    /// 不上传，噎住了
    Choke,
    /// 上传
    UnChoke,
    /// 下载，感兴趣
    Interested,
    /// 不下载
    NotInterested,
    /// 通知对方有了某个片
    Have {
        index:  u32,
    },
    /// 发送位域给对方
    BitField {
        bit_field:  Vec<u8>,
    },
    /// 请求下载
    Request {
        /// 分片序号
        index:  u32,
        /// 偏移
        begin:  u32,
        length: u32,
    },
    /// 发送片
    Piece {
        /// 分片序号
        index:  u32,
        /// 偏移
        begin:  u32,
        data:   Vec<u8>,
    },
    /// 取消 Request 请求
    Cancel {
        index:  u32,
        begin:  u32,
        length: u32,
    },
}

impl PeerMsg {
    pub async fn parse(c: &[u8]) -> anyhow::Result<Self> {
        use PeerMsg::*;
        let mut cur = Cursor::new(c);
        let id = cur.read_u8().await?;
        match id {
            0 => Ok(Choke),
            1 => Ok(UnChoke),
            2 => Ok(Interested),
            3 => Ok(NotInterested),
            4 => {
                Ok(Have { index: cur.read_u32().await? })
            },
            5 => {
                let mut bit_field = Vec::new();
                cur.read_to_end(&mut bit_field).await?;
                Ok(BitField { bit_field })
            },
            6 => {
                Ok(Request {
                    index:  cur.read_u32().await?,
                    begin:  cur.read_u32().await?,
                    length: cur.read_u32().await?,
                })
            },
            7 => {
                let mut data = Vec::new();
                let index = cur.read_u32().await?;
                let begin = cur.read_u32().await?;
                cur.read_to_end(&mut data).await?; 
                Ok(Piece { index, begin, data })
            },
            8 => {
                Ok(Cancel {
                    index:  cur.read_u32().await?,
                    begin:  cur.read_u32().await?,
                    length: cur.read_u32().await?,
                })
            },
            _ => Err(anyhow::Error::msg("PeerMsg parse error")),
        }
    }

    pub fn to_bytes(&self) -> BytesMut {
        let mut buf = BytesMut::new();
        match self {
            PeerMsg::Choke => buf.put_u8(0),
            PeerMsg::UnChoke => buf.put_u8(1),
            PeerMsg::Interested => buf.put_u8(2),
            PeerMsg::NotInterested => buf.put_u8(3),
            PeerMsg::Have { index } => {
                buf.put_u8(4);
                buf.put_u32(*index);
            },
            PeerMsg::BitField { bit_field } => {
                buf.put_u8(5);
                buf.put_slice(&bit_field);
            },
            PeerMsg::Request { index, begin, length } => {
                buf.put_u8(6);
                buf.put_u32(*index);
                buf.put_u32(*begin);
                buf.put_u32(*length);
            },
            PeerMsg::Piece { index, begin, data } => {
                buf.put_u8(7);
                buf.put_u32(*index);
                buf.put_u32(*begin);
                buf.put_slice(&data);
            },
            PeerMsg::Cancel { index, begin, length } => {
                buf.put_u8(8);
                buf.put_u32(*index);
                buf.put_u32(*begin);
                buf.put_u32(*length);
            },
            
        }
        buf
    }

}

#[derive(Debug)]
pub struct Peer {
    pub addr:       SocketAddr,
    pub stream:     Option<TcpStream>,
    pub peer_id:    Option<[u8; 20]>,
    pub info_hash:  Option<[u8; 20]>,
    pub bit_field:  Option<BitField>,
    pub choked:     bool,
}

impl Peer {
    /// 使用 6Bytes 的 ip_port 创建
    pub fn try_new(chunk: &[u8]) -> Result<Self, String> {
        if chunk.len() == 6 {
            let mut peer = Cursor::new(chunk);
            let ip = IpAddr::from([peer.get_u8(), peer.get_u8(), peer.get_u8(), peer.get_u8()]);
            let port = peer.get_u16();
            Ok(Self { 
                addr: SocketAddr::from((ip, port)), 
                stream: None, 
                peer_id: None, 
                info_hash: None, 
                bit_field: None,
                choked: false,
            })
        } else {
            Err("chunk len is not 6".to_owned())
        }
    }

    /// 尝试与对方握手
    /// 
    /// 格式: <pstrlen><pstr><reserved><info_hash><peer_id>
    /// 大小: 1         19      8       20          20
    pub async fn handshake(&mut self, info_hash: &[u8], peer_id: &[u8]) -> anyhow::Result<bool> {
        let mut stream = TcpStream::connect(self.addr).await?;
        
        // 构建握手请求
        let protocol = "BitTorrent protocol";
        let mut req = BytesMut::new();
        req.put_u8(19);
        req.put_slice(protocol.as_bytes());
        req.put_slice(&[0u8; 8]);   // 普通 BT 协议
        req.put_slice(info_hash);
        req.put_slice(peer_id);

        stream.write_all(&req).await?;

        // 读取对方响应
        let len = stream.read_u8().await?;
        let mut r_protocol = [0u8; 19];
        let mut r_info_hash = [0u8; 20];
        let mut r_peer_id = [0u8; 20];

        stream.read_exact(&mut r_protocol).await?;
        stream.read_u64().await?;
        stream.read_exact(&mut r_info_hash).await?;
        stream.read_exact(&mut r_peer_id).await?;

        // 验证
        let status = 
            len == 19
            && r_protocol == protocol.as_bytes()
            && r_info_hash == info_hash;

        if status {
            self.stream = Some(stream);
            self.info_hash = Some(r_info_hash);
            self.peer_id = Some(r_peer_id);
            if let Ok(PeerMsg::BitField { bit_field }) = self.read_msg().await {
                self.bit_field = Some(BitField::new_with(&bit_field))
            } else {
                return Err(anyhow!("recv bitfield error"));
            }
        }

        Ok(status)
    }

    pub async fn send_msg(&mut self, msg: &PeerMsg) -> anyhow::Result<()> {
        if let Some(stream) = self.stream.borrow_mut() {
            let mut buf = BytesMut::new();
            let msg = msg.to_bytes();
            buf.put_u32(msg.len() as u32);
            buf.put_slice(&msg);
            stream.write_all(&buf).await?;
            Ok(())
        } else {
            Err(anyhow::Error::msg("peer is not connect"))
        }
    }

    pub async fn read_msg(&mut self) -> anyhow::Result<PeerMsg> { 
        tokio::time::timeout(Duration::from_secs(30), async {
            if let Some(stream) = self.stream.borrow_mut() {
                let len = stream.read_u32().await?;
                let mut buf = vec![0u8; len as usize];
                stream.read_exact(&mut buf).await?;
                PeerMsg::parse(&buf).await
            } else {
                Err(anyhow::Error::msg("peer is not connect"))
            }
        }).await?
    }

    /// # 处理来自 peer 的响应
    /// - 如果返回的是 Piece 则作为结果返回
    /// - 如果是其他消息则返回空
    #[allow(unused)]
    pub async fn handle_msg(&mut self, msg: PeerMsg) -> anyhow::Result<Option<(u32, Vec<u8>)>> {
        match msg {
            PeerMsg::Choke => self.choked = true,
            PeerMsg::UnChoke => self.choked = false,
            PeerMsg::Have { index } => {
                self.bit_field.as_mut()
                    .expect("bitfiled uninit")
                    .set(index as usize, true);
            },
            PeerMsg::BitField { bit_field } => {
                self.bit_field = Some(BitField::new_with(&bit_field));
            },
            PeerMsg::Piece { index, begin, data } => {
                return Ok(Some((begin, data)));
            },
            PeerMsg::Cancel { index, begin, length } => todo!(),
            _ => (),
        }
        Ok(None)
    }

    /// 执行前需要先握手
    pub async fn routine(
        &mut self, 
        task_receiver: Arc<Mutex<Receiver<PieceTask>>>, 
        task_sender: Sender<PieceTask>,
        result_sender: Sender<PieceResult>
    ) {
        let mut fail_count: usize = 0;
        loop {
            
            // 获取任务
            let task = {
                let mut task_queue = task_receiver.lock().await;
                task_queue.recv().await
            };
            if let Some(task) = task {
                
                if let Some(bitfield) = self.bit_field.borrow_mut() {
                    if bitfield.get(task.index as usize) {
                        // 如果 map 中有这个片
                        if let Ok(result) = self.download_piece(task).await {
                            // 下载成功发送结果
                            result_sender.send(result).await.expect("send piece result error");
                        } else {
                            // 下载失败重新入队
                            fail_count += 1;
                            task_sender.send(task).await.expect("task requeued error");
                        }
                    } else {
                        // 没有这个片
                        // 重新入队
                        task_sender.send(task).await.expect("task requeued error");
                        if fail_count < 100 {
                            fail_count += 1;
                        } else {
                            // 选片失败太多次了，放弃这个 peer
                            println!("close peer: {}", self.addr);
                            return;
                        }
                    }
                } else {
                    panic!("bitfield is not init");
                }
            } else {
                // 没有下载任务结束
                break;
            }
        }
    }

    pub async fn download_piece(&mut self, task: PieceTask) -> anyhow::Result<PieceResult> {
        /// 块大小 16k
        const BLOCK_SIZE: u32 = 1 << 14;
        /// 最大并发量
        const MAX_BACKLOG: usize = 10;

        let mut downloaded = 0; // 已下载字节量
        let mut backlog = 0;    // 并发量（已发送的下载请求量）
        let mut requested = 0;    // 已发送的请求字节量
        let mut buf = vec![0; task.length as usize];

        while downloaded < task.length {
            // 发送下载请求
            if !self.choked {
                // 如果对方允许上传，则发送依次分片请求
                while backlog < MAX_BACKLOG && requested < task.length {
                    // 如果小于最大并发请求量 并 未完成全部字节的请求
                    let request_length = if (task.length - requested) < BLOCK_SIZE {
                        task.length - requested
                    } else {
                        BLOCK_SIZE
                    };
                    self.send_msg(&PeerMsg::Request { index: task.index, begin: requested, length: request_length }).await?;
                    requested += request_length;
                    backlog += 1;
                }
            } else {
                // 对方变为 Choked
                // 发送下载意愿
                self.send_msg(&PeerMsg::Interested).await?;
                // 接收额外的消息
                if let Ok(msg) = self.read_msg().await {
                    self.handle_msg(msg).await.expect("wait unchoke error");
                }
            }

            // 处理响应
            if let Ok(msg) = self.read_msg().await {
                let r = self.handle_msg(msg).await?;
                if let Some((begin, data)) = r {
                    // 将块写入缓冲
                    let end = begin as usize + data.len();
                    buf[begin as usize .. end].copy_from_slice(&data);
                    downloaded += data.len() as u32;
                    backlog -= 1;
                }
            } else {
                return Err(anyhow!("read peer message error"));
            }
        }

        // 校验
        if task.check(&buf) {
            Ok(PieceResult {
                index: task.index,
                data: buf,
            })
        } else {
            println!("hash error");
            Err(anyhow!("hash error"))
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PieceTask {
    /// 分片序号
    pub index:  u32,
    /// 分片长度
    pub length: u32,
    /// SHA-1
    pub hash:   [u8; 20],
}

impl PieceTask {
    pub fn check(&self, piece: &[u8]) -> bool {
        let mut buf = [0u8; 20];
        let mut hasher = Sha1::new();
        hasher.input(piece);
        hasher.result(&mut buf);
        buf == self.hash
    }
}

#[derive(Debug, Clone)]
pub struct PieceResult {
    /// 分片序号
    pub index:  u32,
    /// 分片数据
    pub data:   Vec<u8>,
}
