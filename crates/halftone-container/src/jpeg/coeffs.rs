//! Baseline (sequential Huffman, 8-bit) entropy decoder that recovers the *quantized*
//! DCT coefficients and stops there — no dequantization, no IDCT. Coefficient-domain
//! forensics (double-compression histograms, block-grid alignment) need exactly this
//! and nothing else, and no general image decoder exposes it.
//!
//! Supported: SOF0/SOF1, interleaved and non-interleaved scans, restart intervals,
//! byte stuffing. Rejected with an error: progressive (SOF2), arithmetic coding,
//! 12-bit, lossless. Never panics on hostile input.

use super::tables::HuffSpec;

/// Quantized coefficients of one component, zigzag order per block.
#[derive(Debug, Clone)]
pub struct ComponentCoeffs {
    /// Component id from SOF.
    pub id: u8,
    /// Horizontal sampling factor.
    pub h: u8,
    /// Vertical sampling factor.
    pub v: u8,
    /// Blocks per row in the (MCU-padded) grid.
    pub blocks_w: usize,
    /// Block rows in the (MCU-padded) grid.
    pub blocks_h: usize,
    /// Blocks, row-major, `blocks_w * blocks_h` of them. Quantized, zigzag order.
    pub blocks: Vec<[i16; 64]>,
}

/// All components of a decoded frame.
#[derive(Debug, Clone)]
pub struct Coefficients {
    /// Image width.
    pub width: u16,
    /// Image height.
    pub height: u16,
    /// Components in SOF order.
    pub components: Vec<ComponentCoeffs>,
}

impl Coefficients {
    /// Luma (first) component.
    pub fn luma(&self) -> Option<&ComponentCoeffs> {
        self.components.first()
    }
}

/// Canonical Huffman decoder (T.81 Annex F.2.2.3).
struct HuffDecoder {
    mincode: [i32; 17],
    maxcode: [i32; 18],
    valptr: [i32; 17],
    values: Vec<u8>,
}

impl HuffDecoder {
    fn new(spec: &HuffSpec) -> Result<Self, String> {
        let mut mincode = [0i32; 17];
        let mut maxcode = [-1i32; 18];
        let mut valptr = [0i32; 17];
        let mut code = 0i32;
        let mut k = 0i32;
        for l in 1..=16usize {
            let n = spec.bits[l - 1] as i32;
            if n > 0 {
                valptr[l] = k;
                mincode[l] = code;
                code += n;
                k += n;
                maxcode[l] = code - 1;
            }
            code <<= 1;
        }
        maxcode[17] = i32::MAX;
        if k as usize != spec.values.len() {
            return Err("DHT counts do not match values".into());
        }
        Ok(Self {
            mincode,
            maxcode,
            valptr,
            values: spec.values.clone(),
        })
    }

    fn decode(&self, r: &mut BitReader) -> Result<u8, String> {
        let mut code = 0i32;
        for l in 1..=16usize {
            code = (code << 1) | r.bit()? as i32;
            if self.maxcode[l] >= 0 && code <= self.maxcode[l] {
                let idx = self.valptr[l] + code - self.mincode[l];
                return self
                    .values
                    .get(idx as usize)
                    .copied()
                    .ok_or_else(|| "huffman index out of range".to_string());
            }
        }
        Err("invalid huffman code".into())
    }
}

/// Bit reader over entropy-coded data with 0xFF00 unstuffing and marker detection.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    acc: u32,
    nbits: u32,
    /// Position of a marker we ran into (data is exhausted from here).
    marker_at: Option<usize>,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        Self {
            data,
            pos,
            acc: 0,
            nbits: 0,
            marker_at: None,
        }
    }

    fn fill(&mut self) {
        while self.nbits <= 24 {
            let byte = if self.marker_at.is_some() || self.pos >= self.data.len() {
                0u8 // pad with zeros past the end / past a marker
            } else {
                let b = self.data[self.pos];
                if b == 0xFF {
                    match self.data.get(self.pos + 1) {
                        Some(0x00) => {
                            self.pos += 2;
                            0xFF
                        }
                        _ => {
                            self.marker_at = Some(self.pos);
                            0
                        }
                    }
                } else {
                    self.pos += 1;
                    b
                }
            };
            self.acc |= (byte as u32) << (24 - self.nbits);
            self.nbits += 8;
        }
    }

    fn bit(&mut self) -> Result<u8, String> {
        if self.nbits == 0 {
            self.fill();
        }
        let b = (self.acc >> 31) as u8;
        self.acc <<= 1;
        self.nbits -= 1;
        Ok(b)
    }

    fn receive(&mut self, n: u8) -> Result<u32, String> {
        let mut v = 0u32;
        for _ in 0..n {
            v = (v << 1) | self.bit()? as u32;
        }
        Ok(v)
    }

    fn receive_extend(&mut self, s: u8) -> Result<i32, String> {
        if s == 0 {
            return Ok(0);
        }
        let v = self.receive(s)? as i32;
        Ok(if v < (1 << (s - 1)) {
            v - (1 << s) + 1
        } else {
            v
        })
    }

    /// Byte-align and consume an RSTn marker. Lenient: if the next marker isn't RST
    /// we leave it in place and let the caller finish.
    fn restart(&mut self) -> Result<(), String> {
        self.acc = 0;
        self.nbits = 0;
        let at = match self.marker_at.take() {
            Some(p) => p,
            None => {
                // Scan forward to the next 0xFF that is not a stuffed byte.
                let mut p = self.pos;
                while p + 1 < self.data.len() && !(self.data[p] == 0xFF && self.data[p + 1] != 0) {
                    p += 1;
                }
                p
            }
        };
        if at + 1 < self.data.len()
            && self.data[at] == 0xFF
            && (0xD0..=0xD7).contains(&self.data[at + 1])
        {
            self.pos = at + 2;
            Ok(())
        } else {
            self.pos = at;
            self.marker_at = Some(at);
            Err("expected RST marker".into())
        }
    }

    /// Where the next marker starts (after the entropy data).
    fn end(&self) -> usize {
        match self.marker_at {
            Some(p) => p,
            None => {
                let mut p = self.pos;
                while p + 1 < self.data.len() && !(self.data[p] == 0xFF && self.data[p + 1] != 0) {
                    p += 1;
                }
                p
            }
        }
    }
}

#[derive(Clone, Copy)]
struct FrameComp {
    id: u8,
    h: u8,
    v: u8,
}

/// Decode quantized coefficients from a baseline JPEG.
pub fn decode(b: &[u8]) -> Result<Coefficients, String> {
    if !b.starts_with(&[0xFF, 0xD8]) {
        return Err("not a JPEG".into());
    }
    let mut dc: [Option<HuffDecoder>; 4] = [None, None, None, None];
    let mut ac: [Option<HuffDecoder>; 4] = [None, None, None, None];
    let mut frame: Vec<FrameComp> = Vec::new();
    let (mut width, mut height) = (0u16, 0u16);
    let mut restart_interval = 0usize;
    let mut out: Vec<ComponentCoeffs> = Vec::new();
    let (mut hmax, mut vmax) = (1usize, 1usize);
    let (mut mcus_x, mut mcus_y) = (0usize, 0usize);

    let mut i = 2;
    while i + 1 < b.len() {
        if b[i] != 0xFF {
            return Err(format!("marker expected at {i}"));
        }
        let mut m = i + 1;
        while m < b.len() && b[m] == 0xFF {
            m += 1;
        }
        if m >= b.len() {
            break;
        }
        let marker = b[m];
        i = m + 1;
        match marker {
            0xD9 => break,
            0x01 | 0xD0..=0xD7 => continue,
            _ => {}
        }
        if i + 2 > b.len() {
            return Err("truncated segment".into());
        }
        let len = u16::from_be_bytes([b[i], b[i + 1]]) as usize;
        if len < 2 || i + len > b.len() {
            return Err("bad segment length".into());
        }
        let seg = &b[i + 2..i + len];
        match marker {
            0xC0 | 0xC1 => {
                if seg.len() < 6 || seg[0] != 8 {
                    return Err("only 8-bit baseline/extended-sequential supported".into());
                }
                height = u16::from_be_bytes([seg[1], seg[2]]);
                width = u16::from_be_bytes([seg[3], seg[4]]);
                let n = seg[5] as usize;
                if seg.len() < 6 + 3 * n || n == 0 || width == 0 || height == 0 {
                    return Err("bad SOF".into());
                }
                frame = (0..n)
                    .map(|c| {
                        let o = 6 + 3 * c;
                        FrameComp {
                            id: seg[o],
                            h: (seg[o + 1] >> 4).max(1),
                            v: (seg[o + 1] & 15).max(1),
                        }
                    })
                    .collect();
                hmax = frame.iter().map(|c| c.h as usize).max().unwrap_or(1);
                vmax = frame.iter().map(|c| c.v as usize).max().unwrap_or(1);
                mcus_x = (width as usize).div_ceil(8 * hmax);
                mcus_y = (height as usize).div_ceil(8 * vmax);
                let total_blocks: usize = frame
                    .iter()
                    .map(|c| mcus_x * c.h as usize * mcus_y * c.v as usize)
                    .sum();
                if total_blocks > 8_000_000 {
                    return Err("image too large for coefficient extraction".into());
                }
                out = frame
                    .iter()
                    .map(|c| {
                        let bw = mcus_x * c.h as usize;
                        let bh = mcus_y * c.v as usize;
                        ComponentCoeffs {
                            id: c.id,
                            h: c.h,
                            v: c.v,
                            blocks_w: bw,
                            blocks_h: bh,
                            blocks: vec![[0i16; 64]; bw * bh],
                        }
                    })
                    .collect();
            }
            0xC2 | 0xC6 | 0xCA | 0xCE => return Err("progressive JPEG not supported".into()),
            0xC3 | 0xC7 | 0xCB | 0xCF => return Err("lossless JPEG not supported".into()),
            0xC9 | 0xCD => return Err("arithmetic-coded JPEG not supported".into()),
            0xC4 => {
                let mut p = 0;
                while p + 17 <= seg.len() {
                    let tc = seg[p] >> 4;
                    let th = (seg[p] & 0x0F) as usize;
                    let mut bits = [0u8; 16];
                    bits.copy_from_slice(&seg[p + 1..p + 17]);
                    let n: usize = bits.iter().map(|&x| x as usize).sum();
                    p += 17;
                    if th > 3 || n > 256 || p + n > seg.len() {
                        return Err("bad DHT".into());
                    }
                    let dec = HuffDecoder::new(&HuffSpec {
                        bits,
                        values: seg[p..p + n].to_vec(),
                    })?;
                    if tc == 0 {
                        dc[th] = Some(dec);
                    } else {
                        ac[th] = Some(dec);
                    }
                    p += n;
                }
            }
            0xDD => {
                if seg.len() >= 2 {
                    restart_interval = u16::from_be_bytes([seg[0], seg[1]]) as usize;
                }
            }
            0xDA => {
                if frame.is_empty() {
                    return Err("SOS before SOF".into());
                }
                let ns = *seg.first().ok_or("bad SOS")? as usize;
                if seg.len() < 1 + 2 * ns + 3 {
                    return Err("bad SOS".into());
                }
                // (frame index, dc table, ac table)
                let mut scan: Vec<(usize, usize, usize)> = Vec::with_capacity(ns);
                for k in 0..ns {
                    let cs = seg[1 + 2 * k];
                    let t = seg[2 + 2 * k];
                    let fi = frame
                        .iter()
                        .position(|c| c.id == cs)
                        .ok_or("SOS references unknown component")?;
                    scan.push((fi, (t >> 4) as usize, (t & 15) as usize));
                }
                let ss = seg[1 + 2 * ns];
                let se = seg[2 + 2 * ns];
                if ss != 0 || se != 63 {
                    return Err("non-baseline spectral selection".into());
                }
                for &(_, td, ta) in &scan {
                    if td > 3 || ta > 3 || dc[td].is_none() || ac[ta].is_none() {
                        return Err("SOS references missing Huffman table".into());
                    }
                }
                let mut r = BitReader::new(b, i + len);
                decode_scan(
                    &mut r,
                    &scan,
                    &frame,
                    &mut out,
                    &dc,
                    &ac,
                    restart_interval,
                    hmax,
                    vmax,
                    mcus_x,
                    mcus_y,
                    width,
                    height,
                )?;
                i = r.end();
                continue;
            }
            _ => {}
        }
        i += len;
    }
    if out.is_empty() {
        return Err("no frame decoded".into());
    }
    Ok(Coefficients {
        width,
        height,
        components: out,
    })
}

#[allow(clippy::too_many_arguments)]
fn decode_scan(
    r: &mut BitReader,
    scan: &[(usize, usize, usize)],
    frame: &[FrameComp],
    out: &mut [ComponentCoeffs],
    dc: &[Option<HuffDecoder>; 4],
    ac: &[Option<HuffDecoder>; 4],
    restart_interval: usize,
    hmax: usize,
    vmax: usize,
    mcus_x: usize,
    mcus_y: usize,
    width: u16,
    height: u16,
) -> Result<(), String> {
    let mut preds = [0i32; 4];
    let mut mcu_count = 0usize;

    let restart_check =
        |r: &mut BitReader, preds: &mut [i32; 4], mcu_count: &mut usize| -> Result<bool, String> {
            *mcu_count += 1;
            if restart_interval > 0 && *mcu_count % restart_interval == 0 {
                if r.restart().is_err() {
                    return Ok(false); // stop this scan gracefully
                }
                *preds = [0; 4];
            }
            Ok(true)
        };

    if scan.len() == 1 {
        // Non-interleaved: one block per MCU over the component's own block grid.
        let (fi, td, ta) = scan[0];
        let c = frame[fi];
        let comp_w = (width as usize * c.h as usize).div_ceil(hmax);
        let comp_h = (height as usize * c.v as usize).div_ceil(vmax);
        let bw = comp_w.div_ceil(8);
        let bh = comp_h.div_ceil(8);
        let stride = out[fi].blocks_w;
        let (dcd, acd) = (dc[td].as_ref().ok_or("dc")?, ac[ta].as_ref().ok_or("ac")?);
        for by in 0..bh {
            for bx in 0..bw {
                let blk = decode_block(r, dcd, acd, &mut preds[fi.min(3)])?;
                if let Some(slot) = out[fi].blocks.get_mut(by * stride + bx) {
                    *slot = blk;
                }
                if !restart_check(r, &mut preds, &mut mcu_count)? {
                    return Ok(());
                }
                if r.marker_at.is_some() && r.nbits == 0 {
                    return Ok(());
                }
            }
        }
        return Ok(());
    }

    for my in 0..mcus_y {
        for mx in 0..mcus_x {
            for &(fi, td, ta) in scan {
                let c = frame[fi];
                let (dcd, acd) = (dc[td].as_ref().ok_or("dc")?, ac[ta].as_ref().ok_or("ac")?);
                let stride = out[fi].blocks_w;
                for v in 0..c.v as usize {
                    for h in 0..c.h as usize {
                        let blk = decode_block(r, dcd, acd, &mut preds[fi.min(3)])?;
                        let by = my * c.v as usize + v;
                        let bx = mx * c.h as usize + h;
                        if let Some(slot) = out[fi].blocks.get_mut(by * stride + bx) {
                            *slot = blk;
                        }
                    }
                }
            }
            if !restart_check(r, &mut preds, &mut mcu_count)? {
                return Ok(());
            }
        }
    }
    Ok(())
}

fn decode_block(
    r: &mut BitReader,
    dcd: &HuffDecoder,
    acd: &HuffDecoder,
    pred: &mut i32,
) -> Result<[i16; 64], String> {
    let mut blk = [0i16; 64];
    let t = dcd.decode(r)?;
    let diff = r.receive_extend(t)?;
    *pred += diff;
    blk[0] = (*pred).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    let mut k = 1usize;
    while k < 64 {
        let rs = acd.decode(r)?;
        let run = (rs >> 4) as usize;
        let size = rs & 15;
        if size == 0 {
            if run == 15 {
                k += 16;
                continue;
            }
            break; // EOB
        }
        k += run;
        if k > 63 {
            return Err("AC run past end of block".into());
        }
        blk[k] = r
            .receive_extend(size)?
            .clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        k += 1;
    }
    Ok(blk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_progressive_and_garbage() {
        assert!(decode(b"nope").is_err());
        let mut v = vec![
            0xFF, 0xD8, 0xFF, 0xC2, 0x00, 0x0B, 0x08, 0, 8, 0, 8, 1, 1, 0x11, 0,
        ];
        v.extend_from_slice(&[0xFF, 0xD9]);
        assert!(decode(&v).unwrap_err().contains("progressive"));
    }

    #[test]
    fn huffman_decoder_builds_from_standard_tables() {
        use super::super::tables;
        assert!(HuffDecoder::new(&tables::std_dc_luma()).is_ok());
        assert!(HuffDecoder::new(&tables::std_ac_luma()).is_ok());
        assert!(HuffDecoder::new(&tables::std_ac_chroma()).is_ok());
    }
}
