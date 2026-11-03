use crate::{
    errors::{AppError, Result},
    models::Transcript,
};

pub fn timestamp(ms: u64, separator: char) -> String {
    format!(
        "{:02}:{:02}:{:02}{}{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        separator,
        ms % 1000
    )
}
fn subtitle_text(text: &str, webvtt: bool) -> String {
    let text = text
        .replace('\r', "")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    // Prevent editor text from becoming subtitle markup or cue separators.
    let text = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    if webvtt {
        text
    } else {
        text.replace("--&gt;", "—")
    }
}
pub fn render(transcript: &Transcript, format: &str) -> Result<Vec<u8>> {
    match format {
        "txt" => Ok(transcript.full_text.as_bytes().to_vec()),
        "json" => Ok(serde_json::to_vec_pretty(transcript)?),
        "srt" | "vtt" => {
            let vtt = format == "vtt";
            let mut out = if vtt {
                "WEBVTT\n\n".to_string()
            } else {
                String::new()
            };
            for (i, segment) in transcript.segments.iter().enumerate() {
                let sep = if vtt { '.' } else { ',' };
                out.push_str(&format!(
                    "{}\n{} --> {}\n{}\n\n",
                    i + 1,
                    timestamp(segment.start_ms, sep),
                    timestamp(segment.end_ms, sep),
                    subtitle_text(&segment.text, vtt)
                ));
            }
            Ok(out.into_bytes())
        }
        _ => Err(AppError::new(
            "export_format",
            "Escolha TXT, SRT, VTT ou JSON.",
        )),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timestamps_do_not_wrap_at_24_hours() {
        assert_eq!(timestamp(90_061_007, ','), "25:01:01,007");
    }
    #[test]
    fn cue_text_cannot_inject_markup_or_blocks() {
        assert_eq!(
            subtitle_text("Olá <b>\n\ntexto & voz", true),
            "Olá &lt;b&gt;\ntexto &amp; voz"
        );
    }
}
