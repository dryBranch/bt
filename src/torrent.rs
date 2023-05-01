use crate::bencode::BObject;

/// piece 的长度
const SHA_LEN: usize = 20;

#[derive(Debug, Default)]
pub struct Torrent {
    /// Tracker url
    pub announce:       String,

    // =========info=========
    /// 资源名称
    pub name:           String,
    /// 分片的数量
    pub piece_length:   i32,
    /// 分片的校验码
    pub pieces:         Vec<[u8; SHA_LEN]>,
    /// 具体总大小或信息
    pub length:         i32,
    /// info sha1 特征值
    pub info_hash:      [u8; 20],
    // =========info=========
    
    
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
        let pieces: Vec<_> = info.get_string("pieces")?
            .as_bytes()
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

        Ok(Torrent {
            announce:       value.get_string("announce")?,
            name:           info.get_string("name")?,
            piece_length:   info.get_int("piece length")?,
            pieces,
            length:         info.get_int("length")?,
            info_hash:      info.sha1(),

            comment:        value.get_string("comment").ok(),
            created_by:     value.get_string("create by").ok(),
            creation_date:  value.get_int("creation date").ok(),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use nom::AsBytes;

    use crate::bencode::parse_bobject;

    use super::*;
    
    #[test]
    fn test_all() {
        let s = std::fs::read("test.torrent").unwrap();
        let (_res, obj) = parse_bobject(&s).unwrap();
        std::fs::write("dump.torrent", obj.encode().as_bytes()).unwrap();
        
        let bt = Torrent::try_from(obj).unwrap();
        std::fs::write("parse1.txt", format!("{bt:#?}")).unwrap();
        
    }
}