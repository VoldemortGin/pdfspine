//! Authored 8×8 constant-color JPEGs with numeric component IDs and APP14.
//! No external image asset or encoder is needed. Each component has one DC
//! coefficient and an EOB; quantization is 1, so DC ±512 gives samples 64/192.

pub fn adobe_jpeg(transform: u8, samples: &[u8]) -> Vec<u8> {
    fn segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) {
        out.extend_from_slice(&[0xff, marker]);
        out.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
        out.extend_from_slice(payload);
    }

    let components = u8::try_from(samples.len()).unwrap();
    assert!((1..=4).contains(&components));
    let mut out = vec![0xff, 0xd8];
    let mut adobe = b"Adobe\0\x64\0\0\0\0".to_vec();
    adobe.push(transform);
    segment(&mut out, 0xee, &adobe);
    let mut quant = vec![0];
    quant.extend_from_slice(&[1; 64]);
    segment(&mut out, 0xdb, &quant);
    let mut frame = vec![8, 0, 8, 0, 8, components];
    for id in 1..=components {
        frame.extend_from_slice(&[id, 0x11, 0]);
    }
    segment(&mut out, 0xc0, &frame);
    // DC categories 0 and 10 have codes 00 and 01. AC EOB has code 0.
    let mut huffman = vec![0, 0, 2];
    huffman.extend_from_slice(&[0; 14]);
    huffman.extend_from_slice(&[0, 10, 0x10, 1]);
    huffman.extend_from_slice(&[0; 15]);
    huffman.push(0);
    segment(&mut out, 0xc4, &huffman);
    let mut scan = vec![components];
    for id in 1..=components {
        scan.extend_from_slice(&[id, 0]);
    }
    scan.extend_from_slice(&[0, 63, 0]);
    segment(&mut out, 0xda, &scan);
    let mut bits = String::new();
    for sample in samples {
        bits.push_str(match sample {
            64 => "0101111111110",  // category 10, negative DC -512, EOB
            128 => "000",           // category 0, EOB
            192 => "0110000000000", // category 10, positive DC +512, EOB
            _ => panic!("fixture supports only samples 64, 128, 192"),
        });
    }
    while !bits.len().is_multiple_of(8) {
        bits.push('1');
    }
    for chunk in bits.as_bytes().chunks_exact(8) {
        let byte = chunk.iter().fold(0, |acc, bit| (acc << 1) | (bit - b'0'));
        out.push(byte);
        if byte == 0xff {
            out.push(0);
        }
    }
    out.extend_from_slice(&[0xff, 0xd9]);
    out
}
