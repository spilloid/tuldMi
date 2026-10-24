use tuldmi_engine::dsp::ToMono16k;
use tuldmi_engine::encode::*;
use tuldmi_engine::types::PEAKS_PER_SECOND;

fn sine(freq: f32, rate: u32, secs: f32, amp: f32) -> Vec<f32> {
    let n = (rate as f32 * secs) as usize;
    (0..n).map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin()).collect()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

#[test]
fn ogg_opus_roundtrip_preserves_length_and_energy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a").join("seg.ogg");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let input = sine(440.0, 16_000, 2.37, 0.5);
    let enc = write_ogg_opus(&path, &input).unwrap();

    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..4], b"OggS");
    assert!(bytes.windows(8).any(|w| w == b"OpusHead"));
    assert!(bytes.windows(8).any(|w| w == b"OpusTags"));
    assert_eq!(enc.bytes, bytes.len() as u64);
    assert!(!path.with_extension("ogg.part").exists(), "temp file must be renamed away");
    assert!((enc.duration_ms - 2_370).abs() <= 20, "duration {}", enc.duration_ms);
    // 24 kbps for 2.37 s is ~7 KB; allow container overhead.
    assert!(enc.bytes < 12_000, "file too large: {}", enc.bytes);
    assert_eq!(enc.peaks.len(), (2.37 * PEAKS_PER_SECOND as f32).ceil() as usize);
    assert!(enc.peaks.iter().all(|&p| p > 100), "a 0.5-amplitude sine should have high peaks");

    let decoded = read_ogg_opus_16k(&path).unwrap();
    assert!((decoded.len() as i64 - input.len() as i64).abs() <= 16, "decoded {} vs {}", decoded.len(), input.len());
    let (a, b) = (rms(&input[1600..]), rms(&decoded[1600..decoded.len().min(input.len())]));
    assert!((a - b).abs() / a < 0.2, "energy changed too much: {a} vs {b}");
}

#[test]
fn silence_and_clipping_are_handled() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("s.ogg");
    let enc = write_ogg_opus(&p, &vec![0.0; 8_000]).unwrap();
    assert!(enc.peaks.iter().all(|&v| v < 3));
    let loud: Vec<f32> = sine(200.0, 16_000, 0.5, 3.0); // out of range: must clamp, not panic
    let p2 = dir.path().join("l.ogg");
    let enc2 = write_ogg_opus(&p2, &loud).unwrap();
    assert!(enc2.peaks.iter().any(|&v| v == 255));
}

#[test]
fn peaks_shape() {
    assert!(peaks(&[]).is_empty());
    let one_sec = vec![0.25f32; 16_000];
    let p = peaks(&one_sec);
    assert_eq!(p.len(), PEAKS_PER_SECOND as usize);
    assert!(p.iter().all(|&v| (60..=68).contains(&v)), "0.25 -> ~64: {p:?}");
}

#[test]
fn resampler_preserves_duration_and_frequency_across_odd_chunks() {
    for (rate, ch) in [(48_000u32, 2u16), (44_100, 2), (16_000, 1), (96_000, 1), (22_050, 6)] {
        let mono = sine(1_000.0, rate, 3.0, 0.5);
        let interleaved: Vec<f32> = mono.iter().flat_map(|&s| std::iter::repeat_n(s, ch as usize)).collect();
        let mut r = ToMono16k::new(rate, ch).unwrap();
        let mut out = Vec::new();
        // Feed in awkward chunk sizes (multiples of channel count).
        let mut i = 0;
        let sizes = [7usize, 480, 1, 1023, 4096, 333];
        let mut k = 0;
        while i < interleaved.len() {
            let n = (sizes[k % sizes.len()] * ch as usize).min(interleaved.len() - i);
            r.process(&interleaved[i..i + n], &mut out);
            i += n;
            k += 1;
        }
        r.flush(&mut out);
        let expected = 48_000usize; // 3 s at 16 kHz
        assert!((out.len() as i64 - expected as i64).abs() <= 64, "{rate} Hz/{ch} ch: got {} samples", out.len());
        // Zero crossings ~ 2 * 1000 * 3 = 6000 if the frequency survived.
        let zc = out[800..out.len() - 800].windows(2).filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0)).count();
        let expected_zc = (2.0 * 1_000.0 * (out.len() - 1_600) as f32 / 16_000.0) as i64;
        assert!((zc as i64 - expected_zc).abs() < expected_zc / 50, "{rate} Hz: {zc} zero crossings vs {expected_zc}");
        let level = rms(&out[800..out.len() - 800]);
        assert!((level - 0.3535).abs() < 0.03, "{rate} Hz/{ch} ch: rms {level}");
    }
}
