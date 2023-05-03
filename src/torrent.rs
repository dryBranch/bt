use std::path::Path;

use rand::{thread_rng, Rng};
use reqwest::Client;
use urlencoding::encode_binary;
use anyhow::Error;

use crate::{bencode::{BObject, parse_bobject}, peer::{Peer, PieceTask}};

use std::sync::Arc;

use crate::{peer::{PeerMsg}, bitfield::BitField};
use tokio::sync::{mpsc, Mutex};

/// piece 的长度
const SHA_LEN: usize = 20;

/// # BT 种子信息
/// 兼任客户端，分片最后一段可能小于 piece_length
#[derive(Debug, Default)]
pub struct Torrent {
    /// Tracker url
    pub announce:       String,

    // =========info=========
    /// 资源名称
    pub name:           String,
    /// 分片的数量 Bytes
    pub piece_length:   i32,
    /// 分片的校验码
    pub pieces:         Vec<[u8; SHA_LEN]>,
    /// 具体总大小或信息 Bytes
    pub length:         i32,
    /// info sha1 特征值
    pub info_hash:      [u8; 20],
    // =========info=========
    
    /// 随机生成
    pub peer_id:        [u8; 20],
    // 可选
    /// 备用 Tracker
    pub announce_list:  Option<Vec<String>>,
    /// 备注
    pub comment:        Option<String>,
    /// 创建者
    pub created_by:     Option<String>,
    /// 创建时间
    pub creation_date:  Option<i32>,
    
}

// #[derive(Debug)]
// pub enum Detail {
//     /// 单文件为长度
//     Length(i32),
//     /// 多文件为描述信息
//     Files
// }

impl TryFrom<BObject> for Torrent {
    type Error = String;

    fn try_from(value: BObject) -> Result<Self, Self::Error> {
        let info = value.get("info")?;
        let pieces: Vec<_> = info.get_chunk("pieces")?
            .chunks(20)
            .map(|p| {
                let mut r = [0u8; 20];
                let len = p.len();
                if len != 20 {
                    r[..len].copy_from_slice(p);
                } else {
                    r.clone_from_slice(p);
                }
                r
            } )
            .collect();

        let mut peer_id = [0u8; 20];
        thread_rng().fill(&mut peer_id);

        let announce_list = value.get_list("announce-list").ok()
            .map(|l| {
                l.into_iter()
                    .filter_map(|o| o.to_list().ok() )
                    .map(|o| o.iter().filter_map(|o| o.to_string().ok() ).collect::<Vec<_>>() )
                    .flatten()
                    .collect::<Vec<_>>()
            });


        Ok(Torrent {
            announce:       value.get_string("announce")?,
            name:           info.get_string("name")?,
            piece_length:   info.get_int("piece length")?,
            pieces,
            length:         info.get_int("length")?,
            info_hash:      info.sha1(),
            peer_id,

            announce_list,
            comment:        value.get_string("comment").ok(),
            created_by:     value.get_string("create by").ok(),
            creation_date:  value.get_int("creation date").ok(),
            ..Default::default()
        })
    }
}

impl Torrent {
    pub fn try_new(input: &[u8]) -> anyhow::Result<Self> {
        let (_res, obj) = parse_bobject(input)
            .map_err(|e| Error::msg(format!("can't create BObject for Torrent: {}", e.to_string())))?;
        Self::try_from(obj).map_err(|e| Error::msg(e))
    }

    /// 从文件中读取
    pub async fn try_new_with<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let b = tokio::fs::read(&path).await
            .map_err(|e| Error::msg(format!("can't read {:?}: {}", path.as_ref(), e.to_string())))?;
        Self::try_new(&b)
    }

    /// 从 Tracker 中得到 Peers
    pub async fn get_peers(&self) -> Result<Vec<Peer>, String> {
        let mut peers = Vec::new();
        if let Ok(r) = self.get_peers_url(&self.announce).await {
            peers.extend(r);       
        }

        if let Some(urls) = &self.announce_list {
            for url in urls {
                if let Ok(r) = self.get_peers_url(url).await {
                    peers.extend(r);       
                }
            }
        }
        Ok(peers)
    }

    pub async fn get_peers_url(&self, url: &str) -> Result<Vec<Peer>, String> {
        // 构建参数
        let info_hash = encode_binary(&self.info_hash);
        let peer_id = encode_binary(&self.peer_id);
        let port = "7770";

        // 编码
        let req = format!("{url}?info_hash={info_hash}&peer_id={peer_id}&port={port}");

        // 发送请求
        let client = Client::new();
        let response = client.get(req)
            .send().await
            .map_err(|e| format!("send request to tracker error: {}", e.to_string()))?;
        let content = response.bytes().await
            .map_err(|e| format!("response bytes error: {}", e.to_string()))?;

        // 解析
        let (_, obj) = parse_bobject(&content)
            .map_err(|e| format!("parse response error: {}", e.to_string()))?;
        let peers = obj.get_chunk("peers")?
            .chunks(6)
            .filter_map(|c| Peer::try_new(c).ok() )
            .collect();

        Ok(peers)
    }

    /// 判断这个片的首尾端点，左闭右开，不做错误处理
    pub fn get_bound(&self, index: usize) -> (usize, usize) {
        let begin = index * self.piece_length as usize;
        let mut end = (index + 1) * self.piece_length as usize;
        if end > self.length as usize {
            end = self.length as usize
        }
        (begin, end)
    }

    pub fn get_tasks(&self, bitfield: Option<&BitField>) -> Vec<PieceTask> {
        let mut tasks = Vec::new();
        if let Some(bf) = bitfield {
            bf.0.iter().enumerate()
                .filter(|(_, s)| *s == false)
                .map(|(i, _)| {
                    let (begin, end) = self.get_bound(i);
                    PieceTask {
                        index: i as u32,
                        length: (end - begin) as u32,
                        hash: self.pieces[i],
                    }
                })
                .for_each(|t| tasks.push(t));
            
        } else {
            for (index, piece) in self.pieces.iter().enumerate() {
                let (begin, end) = self.get_bound(index);
                let task = PieceTask {
                    index: index as u32,
                    length: (end - begin) as u32,
                    hash: *piece,
                };
                tasks.push(task);
            }
        }
        tasks
    }

    pub async fn download(path: &str, bitfield: Option<BitField>, result_queue: Option<Vec<Vec<u8>>>) -> anyhow::Result<Option<(BitField, Vec<Vec<u8>>)>> {
        let bt= Torrent::try_new_with(path).await?;
        let mut bitfield = bitfield.unwrap_or(BitField::new(bt.pieces.len()));
        let mut result_queue = result_queue.unwrap_or(vec![vec![]; bt.pieces.len()]);
        println!("start download {}", bt.name);

        let peers = bt.get_peers().await
            .map_err(|e| anyhow::Error::msg(e))?;
        if peers.is_empty() {
            panic!("can't find any peer");
        } else {
            println!("find {} peers", peers.len());
        }

        // 文件分片下载任务队列，无法完成则重新入队
        let (task_sender, task_receiver) = mpsc::channel(bt.pieces.len());
        let task_receiver = Arc::new(Mutex::new(task_receiver));

        // 结果队列
        let (result_sender, mut result_receiver) = mpsc::channel(bt.pieces.len());

        // 将任务入队
        for task in bt.get_tasks(Some(&bitfield)) {
            task_sender.send(task).await.expect("send task into queue error");
        }

        // 开启 peers 下载
        for mut peer in peers {
            let task_receiver = task_receiver.clone();
            let task_sender = task_sender.clone();
            let result_sender = result_sender.clone();
            
            tokio::spawn(async move {
                // 握手
                if let Ok(status) = peer.handshake(&bt.info_hash, &bt.peer_id).await {
                    if status {
                        println!("handshake ok : {}", peer.addr);
                        // 发送下载意愿
                        peer.send_msg(&PeerMsg::Interested).await.expect("send interested fail");
                        // 执行下载
                        peer.routine(task_receiver, task_sender, result_sender).await;
                    }
                }
            });
        }

        // 接收结果
        let mut n = bitfield.count() as f64; // 接收到的片个数
        
        while let Some(piece) = result_receiver.recv().await {
            result_queue[piece.index as usize] = piece.data;
            n += 1.0;
            bitfield.set(piece.index as usize, true);
            println!("got index: {}, count: {}", piece.index, bitfield.count());
            println!("downloaded: {:.2}%", 100.0 * n / bt.pieces.len() as f64);
        }

        if bitfield.count() == bt.pieces.len() {
            let result = result_queue.iter().cloned().flatten().collect::<Vec<_>>();
            tokio::fs::write(&bt.name, result).await.expect("write file error");
            Ok(None)
        } else {
            Ok(Some((bitfield, result_queue)))
        }
        
        
        
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_torrent() {
        let t = Torrent::try_new_with("test.torrent").await.unwrap();
        tokio::fs::write("t.txt", format!("{t:#?}")).await.unwrap();
        println!("{}", t.pieces.len());
    }

    #[tokio::test]
    async fn test_tasks() {
        let t = Torrent::try_new_with("test.torrent").await.unwrap();
        let tasks = t.get_tasks(None);
        println!("{tasks:?}");
    }

    #[tokio::test]
    async fn test_peers() {
        let t = Torrent::try_new_with("test.torrent").await.unwrap();
        let peers = t.get_peers().await.unwrap();
        println!("{peers:#?}");
    }
}

#[cfg(test)]
mod tests_lab {
    use bytes::{ Buf};
    use nom::AsBytes;
    use rand::Rng;
    use reqwest::Client;
    use urlencoding::encode_binary;
    use std::{net::IpAddr, io::Cursor};

    use crate::bencode::parse_bobject;

    use super::*;
    
    #[test]
    fn test_read() {
        let s = std::fs::read("test.torrent").unwrap();
        let (_res, obj) = parse_bobject(&s).unwrap();
        std::fs::write("dump.torrent", obj.encode().as_bytes()).unwrap();
        
        let bt = Torrent::try_from(obj).unwrap();
        std::fs::write("parse1.txt", format!("{bt:#?}")).unwrap();
    }

    #[tokio::test]
    async fn test_get() {
        let url = "http://tr.bangumi.moe:6969/announce";
        let hash = [
            193, 128, 249, 2, 135, 125, 193, 53, 170, 147,
            47, 177, 51, 51, 66, 5, 109, 17, 143, 92,
        ];
        let mut peer_id = [0u8; 20];
        rand::thread_rng().fill(&mut peer_id);

        let hash = encode_binary(&hash);
        let peer_id = encode_binary(&peer_id);
        let port = "17777";
        
        
        let req = format!("{url}?info_hash={hash}&peer_id={peer_id}&port={port}");
        println!("{req}");

        let client = Client::new();
        let res = client.get(req)
            .send().await
            .unwrap();

        let content = res.bytes().await.unwrap();
        let (_, c) = parse_bobject(content.as_bytes()).unwrap();
        println!("{c:#?}");



    }

    #[test]
    fn test_ip_port() {
        let mut peer = Cursor::new(&[127u8, 0, 0, 1, 0, 80]);
        
        let ip = IpAddr::from([peer.get_u8(), peer.get_u8(), peer.get_u8(), peer.get_u8()]);
        let port = peer.get_u16();
        println!("{ip}:{port}");
    }
}