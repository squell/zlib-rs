use core::arch::x86_64::__m128i;
use std::arch::x86_64::{
    _mm_and_si128, _mm_clmulepi64_si128, _mm_extract_epi32, _mm_load_si128, _mm_loadu_si128,
    _mm_or_si128, _mm_shuffle_epi8, _mm_slli_si128, _mm_srli_si128, _mm_storeu_si128,
    _mm_xor_si128,
};

const CRC32_INITIAL_VALUE: u32 = 0;

#[derive(Debug)]
#[repr(C, align(16))]
struct Align16<T>(T);

#[derive(Debug)]
#[repr(C, align(32))]
struct Align32<T>(T);

#[derive(Debug)]
pub struct Crc32Fold {
    #[cfg(target_arch = "x86_64")]
    fold: Accumulator,
    value: u32,
}

impl Crc32Fold {
    pub fn new() -> Self {
        Self {
            #[cfg(target_arch = "x86_64")]
            fold: Accumulator::new(),
            value: Default::default(),
        }
    }

    fn is_pclmulqdq() -> bool {
        is_x86_feature_detected!("pclmulqdq")
            && is_x86_feature_detected!("sse2")
            && is_x86_feature_detected!("sse4.1")
    }

    pub fn fold(&mut self, src: &[u8], start: u32) {
        #[cfg(target_arch = "x86_64")]
        if Self::is_pclmulqdq() {
            return self.fold.fold(src, start);
        }

        // in this case the start value is ignored
        self.value = crc32_braid(src, self.value);
    }

    pub fn fold_copy(&mut self, dst: &mut [u8], src: &[u8]) {
        #[cfg(target_arch = "x86_64")]
        if Self::is_pclmulqdq() {
            return self.fold.fold_copy(dst, src);
        }

        self.value = crc32_braid(src, self.value);
        dst[..src.len()].copy_from_slice(src);
    }

    pub fn finish(self) -> u32 {
        #[cfg(target_arch = "x86_64")]
        if Self::is_pclmulqdq() {
            return unsafe { self.fold.finish() };
        }

        self.value
    }
}

#[cfg(target_arch = "x86_64")]
const fn reg(input: [u32; 4]) -> __m128i {
    // safety: any valid [u32; 4] represents a valid __m128i
    unsafe { core::mem::transmute(input) }
}

#[derive(Debug)]
#[cfg(target_arch = "x86_64")]
struct Accumulator {
    fold: [__m128i; 4],
}

#[cfg(target_arch = "x86_64")]
impl Accumulator {
    const XMM_FOLD4: __m128i = reg([0xc6e41596u32, 0x00000001u32, 0x54442bd4u32, 0x00000001u32]);

    pub const fn new() -> Self {
        let xmm_crc0 = reg([0x9db42487, 0, 0, 0]);
        let xmm_zero = reg([0, 0, 0, 0]);

        Self {
            fold: [xmm_crc0, xmm_zero, xmm_zero, xmm_zero],
        }
    }

    fn fold(&mut self, src: &[u8], start: u32) {
        unsafe { self.fold_help::<false>(&mut [], src, start) }
    }

    fn fold_copy(&mut self, dst: &mut [u8], src: &[u8]) {
        unsafe { self.fold_help::<true>(dst, src, 0) }
    }

    #[target_feature(enable = "pclmulqdq", enable = "sse2", enable = "sse4.1")]
    pub unsafe fn finish(self) -> u32 {
        const CRC_MASK1: __m128i =
            reg([0xFFFFFFFFu32, 0xFFFFFFFFu32, 0x00000000u32, 0x00000000u32]);

        const CRC_MASK2: __m128i =
            reg([0x00000000u32, 0xFFFFFFFFu32, 0xFFFFFFFFu32, 0xFFFFFFFFu32]);

        const RK1_RK2: __m128i = reg([
            0xccaa009e, 0x00000000, /* rk1 */
            0x751997d0, 0x00000001, /* rk2 */
        ]);

        const RK5_RK6: __m128i = reg([
            0xccaa009e, 0x00000000, /* rk5 */
            0x63cd6124, 0x00000001, /* rk6 */
        ]);

        const RK7_RK8: __m128i = reg([
            0xf7011640, 0x00000001, /* rk7 */
            0xdb710640, 0x00000001, /* rk8 */
        ]);

        let [mut xmm_crc0, mut xmm_crc1, mut xmm_crc2, mut xmm_crc3] = self.fold;

        /*
         * k1
         */
        let mut crc_fold = RK1_RK2;

        let x_tmp0 = _mm_clmulepi64_si128(xmm_crc0, crc_fold, 0x10);
        xmm_crc0 = _mm_clmulepi64_si128(xmm_crc0, crc_fold, 0x01);
        xmm_crc1 = _mm_xor_si128(xmm_crc1, x_tmp0);
        xmm_crc1 = _mm_xor_si128(xmm_crc1, xmm_crc0);

        let x_tmp1 = _mm_clmulepi64_si128(xmm_crc1, crc_fold, 0x10);
        xmm_crc1 = _mm_clmulepi64_si128(xmm_crc1, crc_fold, 0x01);
        xmm_crc2 = _mm_xor_si128(xmm_crc2, x_tmp1);
        xmm_crc2 = _mm_xor_si128(xmm_crc2, xmm_crc1);

        let x_tmp2 = _mm_clmulepi64_si128(xmm_crc2, crc_fold, 0x10);
        xmm_crc2 = _mm_clmulepi64_si128(xmm_crc2, crc_fold, 0x01);
        xmm_crc3 = _mm_xor_si128(xmm_crc3, x_tmp2);
        xmm_crc3 = _mm_xor_si128(xmm_crc3, xmm_crc2);

        /*
         * k5
         */
        crc_fold = RK5_RK6;

        xmm_crc0 = xmm_crc3;
        xmm_crc3 = _mm_clmulepi64_si128(xmm_crc3, crc_fold, 0);
        xmm_crc0 = _mm_srli_si128(xmm_crc0, 8);
        xmm_crc3 = _mm_xor_si128(xmm_crc3, xmm_crc0);

        xmm_crc0 = xmm_crc3;
        xmm_crc3 = _mm_slli_si128(xmm_crc3, 4);
        xmm_crc3 = _mm_clmulepi64_si128(xmm_crc3, crc_fold, 0x10);
        xmm_crc3 = _mm_xor_si128(xmm_crc3, xmm_crc0);
        xmm_crc3 = _mm_and_si128(xmm_crc3, CRC_MASK2);

        /*
         * k7
         */
        xmm_crc1 = xmm_crc3;
        xmm_crc2 = xmm_crc3;
        crc_fold = RK7_RK8;

        xmm_crc3 = _mm_clmulepi64_si128(xmm_crc3, crc_fold, 0);
        xmm_crc3 = _mm_xor_si128(xmm_crc3, xmm_crc2);
        xmm_crc3 = _mm_and_si128(xmm_crc3, CRC_MASK1);

        xmm_crc2 = xmm_crc3;
        xmm_crc3 = _mm_clmulepi64_si128(xmm_crc3, crc_fold, 0x10);
        xmm_crc3 = _mm_xor_si128(xmm_crc3, xmm_crc2);
        xmm_crc3 = _mm_xor_si128(xmm_crc3, xmm_crc1);

        !(_mm_extract_epi32(xmm_crc3, 2) as u32)
    }

    fn fold_step<const N: usize>(&mut self) {
        self.fold = std::array::from_fn(|i| match self.fold.get(i + N) {
            Some(v) => *v,
            None => unsafe { Self::step(self.fold[(i + N) - 4]) },
        });
    }

    #[inline(always)]
    unsafe fn step(input: __m128i) -> __m128i {
        _mm_xor_si128(
            _mm_clmulepi64_si128(input, Self::XMM_FOLD4, 0x01),
            _mm_clmulepi64_si128(input, Self::XMM_FOLD4, 0x10),
        )
    }

    unsafe fn partial_fold(&mut self, xmm_crc_part: __m128i, len: usize) {
        const PSHUFB_SHF_TABLE: [__m128i; 15] = [
            reg([0x84838281, 0x88878685, 0x8c8b8a89, 0x008f8e8d]), /* shl 15 (16 - 1)/shr1 */
            reg([0x85848382, 0x89888786, 0x8d8c8b8a, 0x01008f8e]), /* shl 14 (16 - 3)/shr2 */
            reg([0x86858483, 0x8a898887, 0x8e8d8c8b, 0x0201008f]), /* shl 13 (16 - 4)/shr3 */
            reg([0x87868584, 0x8b8a8988, 0x8f8e8d8c, 0x03020100]), /* shl 12 (16 - 4)/shr4 */
            reg([0x88878685, 0x8c8b8a89, 0x008f8e8d, 0x04030201]), /* shl 11 (16 - 5)/shr5 */
            reg([0x89888786, 0x8d8c8b8a, 0x01008f8e, 0x05040302]), /* shl 10 (16 - 6)/shr6 */
            reg([0x8a898887, 0x8e8d8c8b, 0x0201008f, 0x06050403]), /* shl  9 (16 - 7)/shr7 */
            reg([0x8b8a8988, 0x8f8e8d8c, 0x03020100, 0x07060504]), /* shl  8 (16 - 8)/shr8 */
            reg([0x8c8b8a89, 0x008f8e8d, 0x04030201, 0x08070605]), /* shl  7 (16 - 9)/shr9 */
            reg([0x8d8c8b8a, 0x01008f8e, 0x05040302, 0x09080706]), /* shl  6 (16 -10)/shr10*/
            reg([0x8e8d8c8b, 0x0201008f, 0x06050403, 0x0a090807]), /* shl  5 (16 -11)/shr11*/
            reg([0x8f8e8d8c, 0x03020100, 0x07060504, 0x0b0a0908]), /* shl  4 (16 -12)/shr12*/
            reg([0x008f8e8d, 0x04030201, 0x08070605, 0x0c0b0a09]), /* shl  3 (16 -13)/shr13*/
            reg([0x01008f8e, 0x05040302, 0x09080706, 0x0d0c0b0a]), /* shl  2 (16 -14)/shr14*/
            reg([0x0201008f, 0x06050403, 0x0a090807, 0x0e0d0c0b]), /* shl  1 (16 -15)/shr15*/
        ];

        let xmm_shl = PSHUFB_SHF_TABLE[len - 1];
        let xmm_shr = _mm_xor_si128(xmm_shl, reg([0x80808080u32; 4]));

        let xmm_a0 = Self::step(_mm_shuffle_epi8(self.fold[0], xmm_shl));

        self.fold[0] = _mm_shuffle_epi8(self.fold[0], xmm_shr);
        let xmm_tmp1 = _mm_shuffle_epi8(self.fold[1], xmm_shl);
        self.fold[0] = _mm_or_si128(self.fold[0], xmm_tmp1);

        self.fold[1] = _mm_shuffle_epi8(self.fold[1], xmm_shr);
        let xmm_tmp2 = _mm_shuffle_epi8(self.fold[2], xmm_shl);
        self.fold[1] = _mm_or_si128(self.fold[1], xmm_tmp2);

        self.fold[2] = _mm_shuffle_epi8(self.fold[2], xmm_shr);
        let xmm_tmp3 = _mm_shuffle_epi8(self.fold[3], xmm_shl);
        self.fold[2] = _mm_or_si128(self.fold[2], xmm_tmp3);

        self.fold[3] = _mm_shuffle_epi8(self.fold[3], xmm_shr);
        let xmm_crc_part = _mm_shuffle_epi8(xmm_crc_part, xmm_shl);
        self.fold[3] = _mm_or_si128(self.fold[3], xmm_crc_part);

        // zlib-ng uses casts and a floating-point xor instruction here. There is a theory that
        // this breaks dependency chains on some CPUs and gives better throughput. Other sources
        // claim that casting between integer and float has a cost and should be avoided. We can't
        // measure the difference, and choose the shorter code.
        self.fold[3] = _mm_xor_si128(self.fold[3], xmm_a0)
    }

    #[allow(clippy::needless_range_loop)]
    fn progress<const N: usize, const COPY: bool>(
        &mut self,
        dst: &mut [u8],
        src: &mut &[u8],
        init_crc: &mut u32,
    ) -> usize {
        let mut it = src.chunks_exact(16);
        let mut input: [_; 4] = std::array::from_fn(|_| unsafe {
            _mm_load_si128(it.next().unwrap().as_ptr() as *const __m128i)
        });

        *src = &src[N * 16..];

        if COPY {
            for (s, d) in input[..N].iter().zip(dst.chunks_exact(16)) {
                unsafe { _mm_storeu_si128(d.as_ptr() as *mut __m128i, *s) };
            }
        } else if *init_crc != CRC32_INITIAL_VALUE {
            let xmm_initial = reg([*init_crc, 0, 0, 0]);
            input[0] = unsafe { _mm_xor_si128(input[0], xmm_initial) };
            *init_crc = CRC32_INITIAL_VALUE;
        }

        self.fold_step::<N>();

        for i in 0..N {
            self.fold[i + (4 - N)] = unsafe { _mm_xor_si128(self.fold[i + (4 - N)], input[i]) };
        }

        if COPY {
            N * 16
        } else {
            0
        }
    }

    #[target_feature(enable = "pclmulqdq", enable = "sse2", enable = "sse4.1")]
    unsafe fn fold_help<const COPY: bool>(
        &mut self,
        mut dst: &mut [u8],
        mut src: &[u8],
        mut init_crc: u32,
    ) {
        let mut xmm_crc_part = reg([0; 4]);

        let mut partial_buf = Align16([0u8; 16]);

        // Technically the CRC functions don't even call this for input < 64, but a bare minimum of 31
        // bytes of input is needed for the aligning load that occurs.  If there's an initial CRC, to
        // carry it forward through the folded CRC there must be 16 - src % 16 + 16 bytes available, which
        // by definition can be up to 15 bytes + one full vector load. */
        assert!(src.len() >= 31 || init_crc != CRC32_INITIAL_VALUE);

        if COPY {
            assert_eq!(dst.len(), src.len(), "dst and src must be the same length")
        }

        if src.len() < 16 {
            if COPY {
                if src.len() == 0 {
                    return;
                }

                partial_buf.0[..src.len()].copy_from_slice(src);
                xmm_crc_part = _mm_load_si128(partial_buf.0.as_mut_ptr() as *mut __m128i);
                dst[..src.len()].copy_from_slice(&partial_buf.0[..src.len()]);
            }
        } else {
            let align_diff = (16 - (src.as_ptr() as usize & 0xF)) & 0xF;
            if align_diff != 0 {
                xmm_crc_part = _mm_loadu_si128(src.as_ptr() as *const __m128i);
                if COPY {
                    _mm_storeu_si128(dst.as_mut_ptr() as *mut __m128i, xmm_crc_part);
                    dst = &mut dst[align_diff..];
                } else {
                    if init_crc != CRC32_INITIAL_VALUE {
                        let xmm_initial = reg([init_crc, 0, 0, 0]);
                        xmm_crc_part = _mm_xor_si128(xmm_crc_part, xmm_initial);
                        init_crc = CRC32_INITIAL_VALUE;
                    }

                    if align_diff < 4 && init_crc != CRC32_INITIAL_VALUE {
                        let xmm_t0 = xmm_crc_part;
                        xmm_crc_part = _mm_loadu_si128((src.as_ptr() as *const __m128i).add(1));

                        self.fold_step::<1>();

                        self.fold[3] = _mm_xor_si128(self.fold[3], xmm_t0);
                        src = &src[16..];
                    }
                }

                self.partial_fold(xmm_crc_part, align_diff);

                src = &src[align_diff..];
            }

            // if is_x86_feature_detected!("vpclmulqdq") {
            //     if src.len() >= 256 {
            //         if COPY {
            //             // size_t n = fold_16_vpclmulqdq_copy(&xmm_crc0, &xmm_crc1, &xmm_crc2, &xmm_crc3, dst, src, len);
            //             // dst += n;
            //         } else {
            //             // size_t n = fold_16_vpclmulqdq(&xmm_crc0, &xmm_crc1, &xmm_crc2, &xmm_crc3, src, len, xmm_initial, first);
            //             // first = false;
            //         }
            //         // len -= n;
            //         // src += n;
            //     }
            // }

            while src.len() >= 64 {
                let n = self.progress::<4, COPY>(dst, &mut src, &mut init_crc);
                dst = &mut dst[n..];
            }

            if src.len() >= 48 {
                let n = self.progress::<3, COPY>(dst, &mut src, &mut init_crc);
                dst = &mut dst[n..];
            } else if src.len() >= 32 {
                let n = self.progress::<2, COPY>(dst, &mut src, &mut init_crc);
                dst = &mut dst[n..];
            } else if src.len() >= 16 {
                let n = self.progress::<1, COPY>(dst, &mut src, &mut init_crc);
                dst = &mut dst[n..];
            }
        }

        if !src.is_empty() {
            std::ptr::copy_nonoverlapping(
                src.as_ptr(),
                &mut xmm_crc_part as *mut _ as *mut u8,
                src.len(),
            );
            if COPY {
                _mm_storeu_si128(partial_buf.0.as_mut_ptr() as *mut __m128i, xmm_crc_part);
                std::ptr::copy_nonoverlapping(partial_buf.0.as_ptr(), dst.as_mut_ptr(), src.len());
            }

            self.partial_fold(xmm_crc_part, src.len());
        }
    }
}

pub fn crc32(buf: &[u8], start: u32) -> u32 {
    /* For lens < 64, crc32_braid method is faster. The CRC32 instruction for
     * these short lengths might also prove to be effective */
    if buf.len() < 64 {
        return crc32_braid(buf, start);
    }

    let mut crc_state = Crc32Fold::new();
    crc_state.fold(buf, start);
    crc_state.finish()
}

pub fn crc32_copy(dst: &mut [u8], buf: &[u8]) -> u32 {
    /* For lens < 64, crc32_braid method is faster. The CRC32 instruction for
     * these short lengths might also prove to be effective */
    if buf.len() < 64 {
        dst.copy_from_slice(buf);
        return crc32_braid(buf, CRC32_INITIAL_VALUE);
    }

    let mut crc_state = Crc32Fold::new();
    crc_state.fold_copy(dst, buf);
    crc_state.finish()
}

fn crc32_braid(buf: &[u8], start: u32) -> u32 {
    crate::crc32::crc32_braid::<5>(buf, start)
}

#[cfg(test)]
mod test {
    use super::*;

    const INPUT: [u8; 1024] = {
        let mut array = [0; 1024];
        let mut i = 0;
        while i < array.len() {
            array[i] = i as u8;
            i += 1;
        }

        array
    };

    #[test]
    fn test_crc32_fold() {
        // input large enought to trigger the SIMD
        let mut h = crc32fast::Hasher::new_with_initial(CRC32_INITIAL_VALUE);
        h.update(&INPUT);
        assert_eq!(crc32(&INPUT, CRC32_INITIAL_VALUE), h.finalize());
    }

    #[test]
    fn test_crc32_fold_copy() {
        // input large enought to trigger the SIMD
        let mut h = crc32fast::Hasher::new_with_initial(CRC32_INITIAL_VALUE);
        h.update(&INPUT);
        let mut dst = [0; INPUT.len()];
        assert_eq!(crc32_copy(&mut dst, &INPUT), h.finalize());

        assert_eq!(INPUT, dst);
    }

    quickcheck::quickcheck! {
        fn crc_fold_is_crc32fast(v: Vec<u8>, start: u32) -> bool {
            let mut h = crc32fast::Hasher::new_with_initial(start);
            h.update(&v);

            let a = crc32(&v, start) ;
            let b = h.finalize();

            a == b
        }

        fn crc_fold_copy_is_crc32fast(v: Vec<u8>) -> bool {
            let mut h = crc32fast::Hasher::new_with_initial(CRC32_INITIAL_VALUE);
            h.update(&v);

            let mut dst = vec![0; v.len()];

            let a = crc32_copy(&mut dst, &v) ;
            let b = h.finalize();

            assert_eq!(a,b);

            v == dst
        }
    }
}
