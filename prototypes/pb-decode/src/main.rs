// Prototype B (throwaway): the Phase-1 audio path minus subprocess plumbing.
// yt-dlp already resolved the itag-140 m4a URL in the shell (proven there);
// this binary takes that URL as argv[1], then:
// 1. guards the URL (https + host validation, Mimosa constraint),
// 2. fetches it,
// 3. decodes the AAC stream with symphonia,
// 4. runs the PRD's 8-band biquad cascade over real samples, with per-band
//    sine tests verifying the filter math against expected dB.
use biquad::{Biquad, Coefficients, DirectForm1, Type};
use std::error::Error;

// https only, host allowlisted to the expected CDN family, port pinned to 443,
// and the host must RESOLVE to public IPs (blocks DNS-rebinding and intranet
// names). Loopback, private, link-local, CGNAT, reserved, and documentation
// ranges are all rejected. Callers use a no-redirect client, so the guard
// covers every hop.
fn guard_url(raw: &str) -> Result<(String, reqwest::Url), Box<dyn Error>> {
    let url = url::Url::parse(raw)?;
    if url.scheme() != "https" {
        return Err(format!("blocked scheme: {}", url.scheme()).into());
    }
    if url.port_or_known_default() != Some(443) {
        return Err(format!("blocked port: {:?}", url.port()).into());
    }
    let host = url.host_str().ok_or("no host")?;
    let is_ip_literal = host.parse::<std::net::IpAddr>().is_ok();
    let blocked = host.eq_ignore_ascii_case("localhost") || host.ends_with(".local") || is_ip_literal;
    if blocked || !host.ends_with(".googlevideo.com") {
        return Err(format!("blocked host: {host}").into());
    }
    use std::net::ToSocketAddrs;
    let mut resolved = false;
    for addr in (host, 443u16).to_socket_addrs()? {
        resolved = true;
        if !ip_is_public(addr.ip()) {
            return Err(format!("blocked resolved address: {}", addr.ip()).into());
        }
    }
    if !resolved {
        return Err(format!("host did not resolve: {host}").into());
    }
    Ok((host.to_string(), reqwest::Url::parse(raw)?))
}

fn ip_is_public(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            let reserved_band = o[0] & 240 == 240; // 240.0.0.0/4
            let cgnat = o[0] == 100 && (o[1] & 0xC0) == 64; // 100.64.0.0/10
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || cgnat
                || reserved_band)
        }
        std::net::IpAddr::V6(v6) => {
            let s = v6.segments();
            let link_local = (s[0] & 0xffc0) == 0xfe80; // fe80::/10
            let unique_local = (s[0] & 0xfe00) == 0xfc00; // fc00::/7
            !(v6.is_loopback() || v6.is_unspecified() || link_local || unique_local)
        }
    }
}

fn rms_db(xs: &[f32]) -> f64 {
    let s: f64 = xs.iter().map(|x| (*x as f64) * (*x as f64)).sum();
    10.0 * (s / xs.len() as f64).log10()
}

// One band = one biquad; a cascade per channel. Band spec from the PRD.
fn make_band(kind: Type<f32>, freq: f32, rate: f32) -> DirectForm1<f32> {
    DirectForm1::new(
        Coefficients::<f32>::from_params(
            kind,
            biquad::frequency::Hertz::<f32>::from_hz(rate).unwrap(),
            biquad::frequency::Hertz::<f32>::from_hz(freq).unwrap(),
            1.0,
        )
        .unwrap(),
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // The raw argv URL never reaches the client: guard_url parses, allowlists,
    // and resolves it, returning the validated target. The client forbids
    // redirects, so the guard covers every hop of the fetch.
    let raw_arg = std::env::args()
        .nth(1)
        .ok_or("usage: pb_decode <resolved googlevideo url>")?;
    let (host, safe_url) = guard_url(&raw_arg)?;
    println!("[1] URL accepted, host = {host}");

    // 2. Fetch the whole file (3.4 MB, moov may not be at the front).
    let bytes = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?
        .get(safe_url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    println!("[2] fetched {} bytes", bytes.len());
    std::fs::write("track.m4a", &bytes)?;

    // 3. Decode with symphonia (0.6 API: probe -> FormatReader, registry ->
    //    AudioDecoder, GenericAudioBufferRef -> interleaved f32).
    let file = std::fs::File::open("track.m4a")?;
    let mss = symphonia::core::io::MediaSourceStream::new(Box::new(file), Default::default());
    let mut format = symphonia::default::get_probe().probe(
        &symphonia::core::formats::probe::Hint::new(),
        mss,
        Default::default(),
        Default::default(),
    )?;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.as_ref().is_some_and(|p| p.is_audio()))
        .cloned()
        .ok_or("no audio track")?;
    let params = match track.codec_params.as_ref() {
        Some(symphonia::core::codecs::CodecParameters::Audio(a)) => a,
        _ => return Err("not an audio track".into()),
    };
    println!("[3] codec = {:?}, rate = {:?}", params.codec, params.sample_rate);
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &Default::default())?;

    let mut all: Vec<f32> = Vec::new();
    let mut scratch: Vec<f32> = Vec::new();
    let mut spec_rate = params.sample_rate.unwrap_or(44100) as f32;
    let mut nch = 2;
    let mut npackets = 0usize;
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => {
                println!("[3] stream ended cleanly after {npackets} packets");
                break;
            }
            Err(e) => {
                println!("[3] next_packet error after {npackets} packets: {e}");
                break;
            }
        };
        npackets += 1;
        if packet.track_id != track.id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = decoded.spec();
                spec_rate = spec.rate() as f32;
                nch = spec.channels().count();
                // copy_to_vec_interleaved overwrites dst; use a scratch slice
                // and append, so `all` accumulates the whole track.
                scratch.resize(decoded.samples_interleaved(), 0.0);
                decoded.copy_to_slice_interleaved::<f32, _>(scratch.as_mut_slice());
                all.extend_from_slice(&scratch);
            }
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(_) => break,
        }
    }
    let frames = all.len() / nch.max(1);
    println!(
        "[3] decoded {frames} frames, {nch} ch, {spec_rate} Hz, {:.1}s",
        frames as f32 / spec_rate
    );
    if frames < spec_rate as usize {
        return Err("decoded less than 1 second of audio".into());
    }

    // 4a. Per-band verification with pure tones (measure after settling).
    let rate = spec_rate;
    let sine = |f: f32, n: usize| {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * f * i as f32 / rate).sin())
            .collect::<Vec<f32>>()
    };
    let cases = [
        (Type::LowShelf(9.0), 60.0, "band1 60Hz lowshelf +9dB @60Hz tone", 60.0),
        (Type::PeakingEQ(6.0), 1000.0, "band4 1kHz peaking +6dB @1kHz tone", 1000.0),
        (Type::HighShelf(9.0), 16000.0, "band8 16kHz highshelf +9dB @16kHz tone", 16000.0),
    ];
    for (kind, f0, label, tone) in cases {
        let mut x = sine(tone, 1 << 15);
        let warmup = x.len() / 2;
        let before = rms_db(&x[warmup..]);
        let mut band = make_band(kind, f0, rate);
        for v in x.iter_mut() {
            *v = band.run(*v);
        }
        let after = rms_db(&x[warmup..]);
        println!("[4] {label}: {before:.2} dB -> {after:.2} dB (delta {:+.2} dB)", after - before);
    }

    // 4b. Full 8-band cascade over real decoded audio (Vocal-boost preset).
    let spec8: [(Type<f32>, f32); 8] = [
        (Type::LowShelf(4.0), 60.0),
        (Type::PeakingEQ(2.0), 250.0),
        (Type::PeakingEQ(0.0), 500.0),
        (Type::PeakingEQ(-1.0), 1000.0),
        (Type::PeakingEQ(1.5), 2000.0),
        (Type::PeakingEQ(3.0), 4000.0),
        (Type::PeakingEQ(2.0), 8000.0),
        (Type::HighShelf(3.0), 16000.0),
    ];
    let take = 10 * rate as usize * nch; // first 10 s
    let src = &all[..take.min(all.len())];
    let mut cascades: Vec<Vec<DirectForm1<f32>>> = (0..nch)
        .map(|_| spec8.iter().map(|(k, f)| make_band(*k, *f, rate)).collect())
        .collect();
    let mut out_samples = vec![0.0f32; src.len()];
    for i in 0..src.len() {
        let ch = i % nch;
        let mut s = src[i];
        for band in cascades[ch].iter_mut() {
            s = band.run(s);
        }
        out_samples[i] = s;
    }
    println!(
        "[4] 8-band cascade over 10s of real audio: {:.2} dB -> {:.2} dB",
        rms_db(src),
        rms_db(&out_samples)
    );
    println!("PASS");
    Ok(())
}
