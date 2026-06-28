fn parse_filename_metadata(stem: &str) -> FilenameMetadata {
    let normalized = strip_track_number_prefix(stem);
    let parts: Vec<String> = normalized
        .split(" - ")
        .filter_map(clean_metadata_text)
        .collect();

    match parts.as_slice() {
        [artist, title] => FilenameMetadata {
            artist: Some(artist.clone()),
            title: Some(title.clone()),
            album: None,
        },
        [artist, album, title] => FilenameMetadata {
            artist: Some(artist.clone()),
            album: Some(album.clone()),
            title: Some(title.clone()),
        },
        [artist, middle @ .., title] if !middle.is_empty() => FilenameMetadata {
            artist: Some(artist.clone()),
            album: Some(middle.join(" - ")),
            title: Some(title.clone()),
        },
        [title] => FilenameMetadata {
            title: Some(title.clone()),
            ..FilenameMetadata::default()
        },
        _ => FilenameMetadata::default(),
    }
}

fn strip_track_number_prefix(value: &str) -> &str {
    let trimmed = value.trim();
    let digit_end = trimmed
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_digit())
        .map(|(index, ch)| index + ch.len_utf8())
        .last()
        .unwrap_or(0);

    if (1..=3).contains(&digit_end) {
        let rest = trimmed[digit_end..]
            .trim_start()
            .trim_start_matches(['-', '.', '_', ' '])
            .trim_start();
        if !rest.is_empty() {
            return rest;
        }
    }

    trimmed
}

fn clean_metadata_text(value: &str) -> Option<String> {
    let text = value
        .trim_matches('\0')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    (!text.is_empty()).then_some(text)
}

fn format_audio_quality(format: &str, bit_depth: Option<u8>, sample_rate: Option<u32>) -> String {
    let mut label = match bit_depth {
        Some(bits) if bits > 0 => format!("{format} {bits}-bit"),
        _ => format!("{format} Local"),
    };

    if let Some(sample_rate) = sample_rate.and_then(sample_rate_label) {
        label.push_str(" / ");
        label.push_str(&sample_rate);
        if is_dsd_format(format) {
            label.push_str(" PCM");
        }
    }

    label
}

fn format_sample_rate(format: &str, sample_rate: Option<u32>) -> String {
    match sample_rate.and_then(sample_rate_label) {
        Some(mut label) => {
            if is_dsd_format(format) {
                label.push_str(" PCM");
            }
            label
        }
        None => "Unknown".into(),
    }
}

fn sample_rate_label(sample_rate: u32) -> Option<String> {
    if sample_rate == 0 {
        return None;
    }

    if sample_rate >= 1000 {
        let mut khz = if sample_rate.is_multiple_of(1000) {
            format!("{}", sample_rate / 1000)
        } else if sample_rate.is_multiple_of(100) {
            format!("{:.1}", sample_rate as f64 / 1000.0)
        } else {
            format!("{:.3}", sample_rate as f64 / 1000.0)
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string()
        };
        khz.push_str(" kHz");
        return Some(khz);
    }

    Some(format!("{sample_rate} Hz"))
}

fn format_bitrate(bitrate: Option<u32>) -> String {
    match bitrate {
        Some(value) if value > 0 => format!("{value} kbps"),
        _ => "Unknown".into(),
    }
}

fn format_channels(channels: Option<u8>) -> String {
    match channels {
        Some(1) => "Mono".into(),
        Some(2) => "Stereo".into(),
        Some(6) => "5.1".into(),
        Some(8) => "7.1".into(),
        Some(value) if value > 0 => format!("{value} ch"),
        _ => "Unknown".into(),
    }
}

fn format_file_size(bytes: u64) -> String {
    let mb = bytes as f64 / 1024.0 / 1024.0;
    format!("{mb:.1} MB")
}

fn color_pair(hash: u64) -> (String, String) {
    const PAIRS: [(&str, &str); 6] = [
        ("#67e8f9", "#a5b4fc"),
        ("#7dd3fc", "#f0abfc"),
        ("#5eead4", "#93c5fd"),
        ("#f9a8d4", "#86efac"),
        ("#fde68a", "#67e8f9"),
        ("#c4b5fd", "#fda4af"),
    ];
    let pair = PAIRS[(hash as usize) % PAIRS.len()];
    (pair.0.into(), pair.1.into())
}
