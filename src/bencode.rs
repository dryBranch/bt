use ordered_hash_map::OrderedHashMap as HashMap;

use bytes::{BytesMut, BufMut};
use crypto::{sha1::Sha1, digest::Digest};
use nom::{
    IResult, 
    branch::alt, 
    bytes::streaming::{
        take_while1, tag, take
    }, 
    character::is_digit, 
    sequence::Tuple,
    error::{
        Error, ErrorKind
    }
};

/// Bencode 对象
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BObject {
    BSTR(String),
    /// 由于 string 中可能存储非 utf-8 的字节，Debug 输出可能会出错
    BCHUNK(Vec<u8>),
    BINT(i32),
    BLIST(Vec<BObject>),
    BDICT(HashMap<String, BObject>),
}

impl BObject {
    pub fn encode(&self) -> BytesMut {
        match self {
            BObject::BSTR(s) => Self::encode_string(s),
            BObject::BCHUNK(c) => Self::encode_chunk(c),
            BObject::BINT(n) => Self::encode_int(*n),
            BObject::BLIST(list) => Self::encode_list(list),
            BObject::BDICT(dict) => Self::encode_dict(dict),
        }
    }

    pub fn encode_string(val: &str) -> BytesMut {
        let mut buf = BytesMut::new();
        // 字符串长度的 ascii 表示
        let len = val.len().to_string();
        buf.put_slice(len.as_bytes());
        buf.put_slice(b":");
        buf.put_slice(val.as_bytes());
        buf
    }

    pub fn encode_chunk(chunk: &[u8]) -> BytesMut {
        let mut buf = BytesMut::new();
        // 字符串长度的 ascii 表示
        let len = chunk.len().to_string();
        buf.put_slice(len.as_bytes());
        buf.put_slice(b":");
        buf.put_slice(chunk);
        buf
    }

    pub fn encode_int(n: i32) -> BytesMut {
        let mut buf = BytesMut::new();
        buf.put_slice(b"i");
        buf.put_slice(n.to_string().as_bytes());
        buf.put_slice(b"e");
        buf
    }

    pub fn encode_list(list: &Vec<BObject>) -> BytesMut {
        let mut buf = BytesMut::new();
        buf.put_slice(b"l");
        for obj in list {
            buf.put(obj.encode());
        }
        buf.put_slice(b"e");
        buf
    }

    pub fn encode_dict(dict: &HashMap<String, BObject>) -> BytesMut {
        let mut buf = BytesMut::new();
        buf.put_slice(b"d");
        for (key, val) in dict {
            buf.put(Self::encode_string(key));
            buf.put(val.encode());
        }
        buf.put_slice(b"e");
        buf
    }


    // 要是能在解析阶段截取就不用重新序列化了 
    pub fn sha1(&self) -> [u8; 20] {
        let mut r = [0u8; 20];
        let mut hasher = Sha1::new();
        hasher.input(&self.encode());
        hasher.result(&mut r);
        r
    }

    // 快速转换类型
    pub fn to_string(&self) -> Result<String, String> {
        match self {
            BObject::BSTR(s) => Ok(s.clone()),
            _ => Err("not a string".to_owned()),
        }
    }

    pub fn to_chunk(&self) -> Result<Vec<u8>, String> {
        match self {
            BObject::BCHUNK(s) => Ok(s.clone()),
            _ => Err("not a chunk".to_owned()),
        }
    }

    pub fn to_int(&self) -> Result<i32, String> {
        match self {
            BObject::BINT(s) => Ok(*s),
            _ => Err("not a int".to_owned()),
        }
    }

    pub fn to_list(&self) -> Result<Vec<BObject>, String> {
        match self {
            BObject::BLIST(s) => Ok(s.clone()),
            _ => Err("not a list".to_owned()),
        }
    }

    pub fn to_dict(&self) -> Result<HashMap<String, BObject>, String> {
        match self {
            BObject::BDICT(s) => Ok(s.clone()),
            _ => Err("not a dict".to_owned()),
        }
    }

    /// 当作一个 dict 的便捷函数，泛型还不会写
    pub fn get(&self, key: &str) -> Result<BObject, String> {
        match self {
            BObject::BDICT(d) => {
                d.get(key)
                    .map(|o| Ok(o.clone()))
                    .unwrap_or(Err(format!("value of {key} is none")))
            },
            _ => Err("BObject is not a dict".to_owned()),
        }
    }

    pub fn get_string(&self, key: &str) -> Result<String, String> {
        match self {
            BObject::BDICT(d) => {
                d.get(key)
                    .map(|o| match o {
                        BObject::BSTR(n) => Ok(n.clone()),
                        _ => Err(format!("value of {key} is not a string")),
                    })
                    .unwrap_or(Err(format!("value of {key} is none")))
            },
            _ => Err("BObject is not a dict".to_owned()),
        }
    }

    pub fn get_chunk(&self, key: &str) -> Result<Vec<u8>, String> {
        match self {
            BObject::BDICT(d) => {
                d.get(key)
                    .map(|o| match o {
                        BObject::BCHUNK(n) => Ok(n.clone()),
                        _ => Err(format!("value of {key} is not a chunk")),
                    })
                    .unwrap_or(Err(format!("value of {key} is none")))
            },
            _ => Err("BObject is not a dict".to_owned()),
        }
    }

    pub fn get_int(&self, key: &str) -> Result<i32, String> {
        match self {
            BObject::BDICT(d) => {
                d.get(key)
                    .map(|o| match o {
                        BObject::BINT(n) => Ok(*n),
                        _ => Err(format!("value of {key} is not a int")),
                    })
                    .unwrap_or(Err(format!("value of {key} is none")))
            },
            _ => Err("BObject is not a dict".to_owned()),
        }
    }

    pub fn get_list(&self, key: &str) -> Result<Vec<BObject>, String> {
        match self {
            BObject::BDICT(d) => {
                d.get(key)
                    .map(|o| match o {
                        BObject::BLIST(n) => Ok(n.clone()),
                        _ => Err(format!("value of {key} is not a list")),
                    })
                    .unwrap_or(Err(format!("value of {key} is none")))
            },
            _ => Err("BObject is not a dict".to_owned()),
        }
    }

    pub fn get_dict(&self, key: &str) -> Result<HashMap<String, BObject>, String> {
        match self {
            BObject::BDICT(d) => {
                d.get(key)
                    .map(|o| match o {
                        BObject::BDICT(n) => Ok(n.clone()),
                        _ => Err(format!("value of {key} is not a dict")),
                    })
                    .unwrap_or(Err(format!("value of {key} is none")))
            },
            _ => Err("BObject is not a dict".to_owned()),
        }
    }
}

pub fn parse_bobject(input: &[u8]) -> IResult<&[u8], BObject> {
    alt((
        parse_string,
        parse_int,
        parse_list,
        parse_dict
    ))(input)
}

fn parse_string(input: &[u8]) -> IResult<&[u8], BObject> {
    let (input, (len, _sp)) = (parse_decimal, tag(b":")).parse(input)?;
    let (input, s) = take(len as usize)(input)?;
    let s = if let Ok(s) = String::from_utf8(s.to_vec()) {
        BObject::BSTR(s)
    } else {
        BObject::BCHUNK(s.to_vec())
    };
    Ok((input, s))
}

fn parse_int(input: &[u8]) -> IResult<&[u8], BObject> {
    let (input, (_i, n, _e)) = (tag(b"i"), parse_decimal, tag(b"e")).parse(input)?;
    Ok((input, BObject::BINT(n as i32)))
}

fn parse_list(input: &[u8]) -> IResult<&[u8], BObject> {
    let (input, (_l, data, _e)) = (tag(b"l"), parse_seq, tag(b"e")).parse(input)?;
    Ok((input, BObject::BLIST(data)))
}

fn parse_dict(input: &[u8]) -> IResult<&[u8], BObject> {
    let (input, (_d, data, _e)) = (tag(b"d"), parse_key_pairs, tag(b"e")).parse(input)?;
    Ok((input, BObject::BDICT(data)))
}

/// 解析平凡十进制数字
fn parse_decimal(input: &[u8]) -> IResult<&[u8], isize> {
    let (input, n) = take_while1(is_digit)(input)?;
    let n = String::from_utf8_lossy(n).to_string();
    let n = n.parse().unwrap();
    Ok((input, n))
}

/// 依次解析 BObject 序列
fn parse_seq(mut input: &[u8]) -> IResult<&[u8], Vec<BObject>> {
    let mut list = vec![];
    while !input.is_empty() {
        // 可能会解析失败，那么可能是到末尾了或空值则不管他
        let (_input, obj) = match parse_bobject(input) {
            Ok(r) => r,
            Err(_) => break,
        };
        input = _input;
        list.push(obj);
    }
    Ok((input, list))
}

/// 依次解析 BObject 键值对，String: BObject
fn parse_key_pairs(mut input: &[u8]) -> IResult<&[u8], HashMap<String, BObject>> {
    let mut map = HashMap::new();
    while !input.is_empty() {
        // 可能会解析失败，那么可能是到末尾了或空值则不管他
        let (_input, (key, val)) = match (parse_string, parse_bobject).parse(input) {
            Ok(r) => r,
            Err(_) => break,
        };
        input = _input;
        let key = if let BObject::BSTR(k) = key {
            k
        } else {
            return Err(nom::Err::Error(Error { input: b"fdf", code: ErrorKind::Verify }))
        };
        map.insert(key, val);
    }
    Ok((input, map))
}

#[cfg(test)]
mod tests {
    use nom::AsBytes;

    use super::*;
    
    #[test]
    fn test_decimal() {
        let (_, s) = parse_decimal(b"b1234e").unwrap();
        println!("{s:?}");
    }

    #[test]
    fn test_str() {
        let (_, s) = parse_string(b"11:hello world").unwrap();
        println!("{s:?}");
        println!("{:?}", s.encode());
    }

    #[test]
    fn test_int() {
        let input = b"i123ef";
        let (_, s) = parse_int(input).unwrap();
        println!("{s:?}");
        println!("{:?}", s.encode());
    }

    #[test]
    fn test_list() {
        let input = b"l11:hello worldi123eef";
        let (_, s) = parse_list(input).unwrap();
        println!("{s:#?}");
        println!("{:?}", s.encode());
    }

    #[test]
    fn test_dict() {
        let input = b"d11:hello worldi123eef";
        let (_, s) = parse_dict(input).unwrap();
        println!("{s:#?}");
        println!("{:?}", s.encode());
    }

    #[test]
    fn test_all() {
        let s = std::fs::read("test.torrent").unwrap();
        let (_res, obj) = parse_bobject(&s).unwrap();
        std::fs::write("dump.torrent", obj.encode()).unwrap();
        assert!(obj.encode().as_bytes() == s.as_bytes());
        println!("{obj:?}")
    }
}