use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use effectcraft_time::FrameRate;

pub fn configure_process(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

pub fn locate(configured: &str, cli: &Path) -> Result<PathBuf, String> {
    if !configured.trim().is_empty() {
        let path = PathBuf::from(configured);
        return path.is_file().then_some(path).ok_or_else(|| "지정한 FFmpeg 실행 파일이 없습니다".into());
    }
    let name = format!("ffmpeg{}", std::env::consts::EXE_SUFFIX);
    let mut candidates = Vec::new();
    if let Some(parent) = cli.parent() {
        candidates.push(parent.join(&name));
    }
    if let Some(paths) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&paths).filter(|path| !path.as_os_str().is_empty()).map(|path| path.join(&name)));
    }
    if let Some(drive) = std::env::var_os("SystemDrive") {
        candidates.push(PathBuf::from(format!("{}\\FFMPEG\\bin", drive.to_string_lossy())).join(&name));
    }
    for variable in ["ProgramFiles", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(variable) {
            candidates.push(PathBuf::from(base).join("FFmpeg/bin").join(&name));
        }
    }
    candidates.into_iter().find(|path| path.is_file()).ok_or_else(|| "FFmpeg를 찾지 못했습니다. 호스트에 FFmpeg를 설치하거나 실행 파일을 지정하세요".into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoder {
    Nvenc,
    Amf,
    Qsv,
    Cpu,
}

impl Encoder {
    pub fn name(self) -> &'static str {
        match self {
            Self::Nvenc => "h264_nvenc",
            Self::Amf => "h264_amf",
            Self::Qsv => "h264_qsv",
            Self::Cpu => "libx264",
        }
    }
}

pub fn candidates(width: u32, height: u32) -> Vec<Encoder> {
    if !width.is_multiple_of(2) || !height.is_multiple_of(2) { vec![Encoder::Cpu] } else { vec![Encoder::Nvenc, Encoder::Amf, Encoder::Qsv, Encoder::Cpu] }
}

/// Try the actual input and output settings, not just FFmpeg's compiled encoder list.
/// Probes never touch the final output and cannot stall assembly indefinitely.
pub fn rank_encoders(ffmpeg: &Path, video: &Video<'_>, scratch: &Path, mut report: impl FnMut(String)) -> Vec<Encoder> {
    use std::time::{Duration, Instant};
    let candidates = candidates(video.width, video.height);
    if candidates.len() == 1 || video.end - video.start <= 24 {
        report("짧은 영상/홀수 해상도: 추가 비교 비용을 줄이고 실제 인코딩 성공 여부로 선택".into());
        return candidates;
    }
    let output = scratch.join("encoder-probe.mp4");
    let errors = scratch.join("encoder-probe.stderr");
    let sample = Video { end: (video.start + 8).min(video.end), audio: None, output: &output, ..*video };
    let mut measured = Vec::new();
    for encoder in candidates {
        let attempt = (|| -> Result<f64, String> {
            if output.exists() {
                std::fs::remove_file(&output).map_err(|e| e.to_string())?;
            }
            let mut command = command(ffmpeg, &sample, encoder);
            command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::from(std::fs::File::create(&errors).map_err(|e| e.to_string())?));
            let started = Instant::now();
            let mut child = command.spawn().map_err(|e| e.to_string())?;
            loop {
                match child.try_wait() {
                    Ok(Some(status)) if status.success() && std::fs::metadata(&output).is_ok_and(|m| m.len() > 128) => {
                        return Ok(started.elapsed().as_secs_f64());
                    }
                    Ok(Some(_)) => return Err(stderr_tail(&errors)),
                    Ok(None) if started.elapsed() < Duration::from_secs(3) => std::thread::sleep(Duration::from_millis(10)),
                    result => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(result.err().map_or_else(|| "비교 제한 시간 3초 초과".into(), |e| e.to_string()));
                    }
                }
            }
        })();
        match attempt {
            Ok(seconds) => {
                report(format!("{} 샘플 {}프레임: {seconds:.3}초", encoder.name(), sample.end - sample.start));
                measured.push((encoder, seconds));
            }
            Err(error) => report(format!("{} 비교 제외: {error}", encoder.name())),
        }
    }
    measured.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut ranked: Vec<_> = measured.into_iter().map(|(encoder, _)| encoder).collect();
    if !ranked.contains(&Encoder::Cpu) {
        ranked.push(Encoder::Cpu);
    }
    ranked
}

pub struct Video<'a> {
    pub directory: &'a Path,
    pub start: i64,
    pub end: i64,
    pub rate: FrameRate,
    pub width: u32,
    pub height: u32,
    pub pixel_aspect: f64,
    pub bitrate: u32,
    pub audio: Option<&'a Path>,
    pub output: &'a Path,
}

fn command(ffmpeg: &Path, video: &Video<'_>, encoder: Encoder) -> Command {
    let mut command = Command::new(ffmpeg);
    configure_process(&mut command);
    let rate = format!("{}/{}", video.rate.num, video.rate.den);
    // Escape literal percent signs in a user-selected folder, leaving only the frame placeholder.
    let directory = video.directory.to_string_lossy().replace('%', "%%");
    let pattern = PathBuf::from(directory).join("frame_%05d.png");
    command
        .args(["-hide_banner", "-nostdin", "-loglevel", "warning", "-y", "-progress", "pipe:1", "-nostats"])
        .arg("-filter_threads")
        .arg(crate::resources::Resources::detect().threads(video.width, video.height).to_string())
        .arg("-framerate")
        .arg(&rate)
        .arg("-start_number")
        .arg(video.start.to_string())
        .arg("-i")
        .arg(pattern);
    if let Some(audio) = video.audio {
        command.arg("-i").arg(audio);
    }
    command.args(["-map", "0:v:0"]);
    if video.audio.is_some() {
        command.args(["-map", "1:a:0", "-c:a", "aac", "-b:a", "192k", "-af", "apad"]);
    } else {
        command.arg("-an");
    }
    let odd = !video.width.is_multiple_of(2) || !video.height.is_multiple_of(2);
    let format = if odd { "yuv444p" } else { "yuv420p" };
    let filter = format!("scale=in_range=full:out_range=tv:out_color_matrix=bt709,setsar={:.9},format={format}", video.pixel_aspect);
    command.arg("-vf").arg(filter).arg("-c:v").arg(encoder.name());
    match encoder {
        Encoder::Nvenc => {
            command.args(["-preset", "p4", "-tune", "hq", "-rc", "vbr", "-multipass", "disabled", "-rc-lookahead", "0"]);
        }
        Encoder::Amf => {
            command.args(["-usage", "transcoding", "-quality", "speed", "-rc", "vbr_peak"]);
        }
        Encoder::Qsv => {
            command.args(["-preset", "veryfast", "-look_ahead", "0"]);
        }
        Encoder::Cpu => {
            command.args(["-preset", "veryfast"]);
            command.arg("-threads").arg(crate::resources::Resources::detect().threads(video.width, video.height).to_string());
        }
    }
    command
        .arg("-b:v")
        .arg(format!("{}k", video.bitrate))
        .arg("-maxrate")
        .arg(format!("{}k", video.bitrate as u64 * 3 / 2))
        .arg("-bufsize")
        .arg(format!("{}k", video.bitrate as u64 * 2))
        .args(["-pix_fmt", format, "-color_range", "tv", "-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709", "-fps_mode", "cfr"])
        .arg("-frames:v")
        .arg((video.end - video.start).to_string())
        .arg("-t")
        .arg(format!("{:.10}", (video.end - video.start) as f64 * video.rate.den as f64 / video.rate.num as f64))
        .arg(video.output);
    command
}

pub fn stderr_tail(path: &Path) -> String {
    (|| -> std::io::Result<String> {
        let mut file = std::fs::File::open(path)?;
        let length = file.metadata()?.len();
        file.seek(SeekFrom::Start(length.saturating_sub(4000)))?;
        let mut bytes = Vec::new();
        file.take(4000).read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
    })()
    .unwrap_or_default()
}

pub fn encode(ffmpeg: &Path, video: &Video<'_>, encoder: Encoder, stderr: &Path, mut progress: impl FnMut(i64)) -> Result<(), String> {
    let mut command = command(ffmpeg, video, encoder);
    let errors = std::fs::File::create(stderr).map_err(|e| e.to_string())?;
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::from(errors));
    let mut child = command.spawn().map_err(|e| format!("FFmpeg 실행: {e}"))?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("FFmpeg 진행률 출력이 없습니다".into());
    };
    for line in BufReader::new(stdout).lines() {
        match line {
            Ok(line) => {
                if let Some(value) = line.strip_prefix("frame=").and_then(|v| v.trim().parse::<i64>().ok()) {
                    progress(value.clamp(0, video.end - video.start));
                }
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("FFmpeg 진행률 읽기: {error}"));
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("{} 종료 {status}: {}", encoder.name(), stderr_tail(stderr)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_input_probes_rank_working_encoders_and_preserve_final_output() {
        let Some(ffmpeg) = std::env::var_os("EFFECTCRAFT_FARM_TEST_FFMPEG").map(PathBuf::from) else { return };
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("farm-encoder-probe-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        for frame in 7..39 {
            image::RgbImage::from_pixel(320, 180, image::Rgb([frame as u8 * 5, 20, 90])).save(root.join(format!("frame_{frame:05}.png"))).unwrap();
        }
        let output = root.join("final.mp4");
        std::fs::write(&output, b"existing output must survive probes").unwrap();
        let video = Video {
            directory: &root,
            start: 7,
            end: 39,
            rate: FrameRate::new(30000, 1001),
            width: 320,
            height: 180,
            pixel_aspect: 1.2,
            bitrate: 16000,
            audio: None,
            output: &output,
        };
        let mut events = Vec::new();
        let ranked = rank_encoders(&ffmpeg, &video, &root, |message| events.push(message));
        assert!(ranked.contains(&Encoder::Cpu));
        assert_eq!(events.len(), 4);
        let selected = ranked[0];
        assert!(events.iter().any(|message| message.starts_with(&format!("{} 샘플", selected.name()))));
        assert_eq!(std::fs::read(&output).unwrap(), b"existing output must survive probes");
        let encoded = root.join("selected.mp4");
        encode(&ffmpeg, &Video { output: &encoded, ..video }, selected, &root.join("selected.stderr"), |_| {}).unwrap();
        assert!(std::fs::metadata(encoded).unwrap().len() > 128);
        assert!(root.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preserves_rational_rate_range_and_audio_mapping() {
        let video = Video {
            directory: Path::new("frames%test"),
            start: 120,
            end: 180,
            rate: FrameRate::new(30000, 1001),
            width: 1920,
            height: 1080,
            pixel_aspect: 1.2,
            bitrate: 16000,
            audio: Some(Path::new("mix.wav")),
            output: Path::new("out.mp4"),
        };
        let command = command(Path::new("ffmpeg"), &video, Encoder::Nvenc);
        let args: Vec<_> = command.get_args().map(|s| s.to_string_lossy().into_owned()).collect();
        assert!(args.windows(2).any(|v| v == ["-framerate", "30000/1001"]));
        assert!(args.windows(2).any(|v| v == ["-start_number", "120"]));
        assert!(args.windows(2).any(|v| v == ["-frames:v", "60"]));
        assert!(args.windows(2).any(|v| v == ["-c:v", "h264_nvenc"]));
        assert!(args.windows(2).any(|v| v == ["-map", "1:a:0"]));
        assert!(args.iter().any(|s| s.contains("frames%%test")));
        assert!(!args.iter().any(|s| s == "-hwaccel")); // PNG decoding is not an NVDEC workload.
    }

    #[test]
    fn real_encoders_preserve_fractional_fps_dimensions_and_selected_frames() {
        let Some(ffmpeg) = std::env::var_os("EFFECTCRAFT_FARM_TEST_FFMPEG").map(PathBuf::from) else { return };
        let ffprobe = ffmpeg.parent().unwrap().join(format!("ffprobe{}", std::env::consts::EXE_SUFFIX));
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("farm-encoding-{}-{nonce} % 한글", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        for (encoder, width, height) in [(Encoder::Nvenc, 320, 180), (Encoder::Cpu, 321, 181)] {
            if encoder == Encoder::Nvenc && std::env::var_os("EFFECTCRAFT_FARM_TEST_NVENC").is_none() {
                continue;
            }
            let frames = root.join(encoder.name());
            std::fs::create_dir(&frames).unwrap();
            for frame in 7..13 {
                image::RgbImage::from_pixel(width, height, image::Rgb([frame as u8 * 11, 20, 90])).save(frames.join(format!("frame_{frame:05}.png"))).unwrap();
            }
            let output = frames.join("out.mp4");
            let video = Video {
                directory: &frames,
                start: 7,
                end: 13,
                rate: FrameRate::new(30000, 1001),
                width,
                height,
                pixel_aspect: 1.2,
                bitrate: 16000,
                audio: None,
                output: &output,
            };
            let mut done = 0;
            encode(&ffmpeg, &video, encoder, &frames.join("stderr.log"), |value| done = value).unwrap();
            assert_eq!(done, 6);
            let result = Command::new(&ffprobe).args(["-v", "error", "-count_frames", "-show_streams", "-of", "json"]).arg(&output).output().unwrap();
            assert!(result.status.success());
            let json: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
            let stream = &json["streams"][0];
            assert_eq!(stream["codec_name"], "h264");
            assert_eq!(stream["width"], width);
            assert_eq!(stream["height"], height);
            assert_eq!(stream["r_frame_rate"], "30000/1001");
            assert_eq!(stream["nb_read_frames"], "6");
            assert_eq!(stream["sample_aspect_ratio"], "6:5");
            assert_eq!(stream["color_space"], "bt709");
            assert!((stream["duration"].as_str().unwrap().parse::<f64>().unwrap() - 0.2002).abs() < 0.001);
            let decoded = Command::new(&ffmpeg)
                .args(["-v", "error", "-i"])
                .arg(&output)
                .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"])
                .output()
                .unwrap();
            assert!(decoded.status.success());
            assert_eq!(decoded.stdout.len(), (width * height * 3) as usize);
            for (actual, expected) in decoded.stdout[..3].iter().zip([77_i16, 20, 90]) {
                assert!((i16::from(*actual) - expected).abs() <= 6);
            }
        }
        assert!(root.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::remove_dir_all(root).unwrap();
    }
}
