use dyn_bitmap::DynBitmap;

#[derive(Debug)]
pub struct BitField(pub DynBitmap);

impl BitField {
    pub fn new(len: usize) -> Self {
        Self(vec![false; len].into_iter().collect())
    }

    pub fn new_with(bs: &[u8]) -> Self {
        Self(bs.iter()
            .flat_map(|n| [
                n & 0b1,
                n & 0b10,
                n & 0b100,
                n & 0b1000,
                n & 0b10000,
                n & 0b100000,
                n & 0b1000000,
                n & 0b10000000,
            ] )
            .map(|n| n != 0)
            .collect())
    }

    pub fn get(&self, index: usize) -> bool {
        if let Ok(s) = self.0.get(index) {
            s
        } else {
            false
        }
    }

    pub fn set(&mut self, index: usize, val: bool) {
        if let Ok(_) = self.0.set(index, val) { }
    }

    pub fn count(&self) -> usize {
        let mut n = 0;
        for s in self.0.iter() {
            if s {
                n += 1;
            }
        }
        return n;
    }
}


// /// # 位域
// /// 
// /// 偏移从低位到高位计算
// #[derive(Debug, Clone, Default)]
// pub struct BitField {
//     pub len:    usize,
//     pub inner:  Vec<u8>,
// }

// impl BitField {
//     pub fn new(len: usize) -> Self {
//         let byte_len = (len >> 3) + if (len & 0x07) == 0 { 0 } else { 1 };
//         Self {
//             len,
//             inner: vec![0u8; byte_len],
//         }
//     }

//     pub fn new_with(bs: &[u8]) -> Self {
//         let mut bf = Self::new(bs.len() << 3);
//         bf.inner.copy_from_slice(bs);
//         bf
//     }

//     /// # 判断位状态
//     /// 
//     /// 不返回越界判断，如果越界返回 false
//     pub fn get(&self, index: usize) -> bool {
//         if index < self.len {
//             let b_index = index >> 3;   // 判断在 vec 的哪个 Byte 里
//             let bias = index & 0x07;    // 偏移
//             ( self.inner[b_index] & (0x1 << bias) ) != 0
//         } else {
//             false
//         }
//     }

//     /// # 置位
//     /// 
//     /// 不做越界扩容，如果越界则当作无事发生
//     pub fn set(&mut self, index: usize, val: bool) {
//         if index < self.len {
//             let b_index = index >> 3;   // 判断在 vec 的哪个 Byte 里
//             let bias = index & 0x07;    // 偏移
//             if val {
//                 self.inner[b_index] |= 0x1 << bias;     // 0b10 | 0b01 = 0b11
//             } else {
//                 self.inner[b_index] &= !(0x1 << bias);  // 0b11 & (!0b10) = 0b01
//             }
//         }
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test1() {
        let mut m = BitField::new(11);
        m.set(10, true);
        assert!(m.get(9) == false);
        assert!(m.get(10) == true);
        assert!(m.get(11) == false);

        m.set(10, false);
        assert!(m.get(9) == false);
        assert!(m.get(10) == false);
        assert!(m.get(11) == false);
        println!("{m:?}");
    }
}