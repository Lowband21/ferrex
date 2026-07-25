#!/usr/bin/env python3
"""Generate and verify the synthetic desktop playback acceptance fixtures.

The output is deliberately written below ``target/`` by default and is not
committed.  Every media stream starts from FFmpeg lavfi sources; the only
external input is a user-selected font embedded in the ASS/attachment fixtures.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shlex
import shutil
import struct
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable, Sequence

SCHEMA_VERSION = 1
MARKER_NAME = ".ferrex-native-playback-fixtures"
DEFAULT_OUTPUT = Path("target/native-playback-fixtures")
DURATION_SECONDS = 4
VIDEO_SIZE = "640x360"
FRAME_RATE = 24

SDR_FILTER = (
    "format=yuv420p,"
    "setparams=range=limited:color_primaries=bt709:"
    "color_trc=bt709:colorspace=bt709"
)
SDR_10_FILTER = (
    "format=yuv420p10le,"
    "setparams=range=limited:color_primaries=bt709:"
    "color_trc=bt709:colorspace=bt709"
)
PQ_FILTER = (
    "format=yuv420p10le,"
    "setparams=range=limited:color_primaries=bt2020:"
    "color_trc=smpte2084:colorspace=bt2020nc"
)
HLG_FILTER = (
    "format=yuv420p10le,"
    "setparams=range=limited:color_primaries=bt2020:"
    "color_trc=arib-std-b67:colorspace=bt2020nc"
)

PRIMARY_FIXTURES = (
    "h264-sdr-8bit.mkv",
    "hevc-main10-sdr.mkv",
    "hdr10-pq.mkv",
    "hlg.mkv",
    "vp9-sdr.mkv",
    "av1-sdr.mkv",
    "ass-animation-fonts.mkv",
    "pgs-bitmap.mkv",
    "multitrack-structure.mkv",
)


class FixtureError(RuntimeError):
    """Actionable fixture generation or validation failure."""


@dataclass(frozen=True)
class Tools:
    ffmpeg: str
    ffprobe: str


@dataclass(frozen=True)
class BuildContext:
    root: Path
    tools: Tools
    font: Path

    @property
    def sources(self) -> Path:
        return self.root / "sources"


def run(
    command: Sequence[os.PathLike[str] | str],
    *,
    cwd: Path | None = None,
    capture: bool = False,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    argv = [os.fspath(part) for part in command]
    if not capture:
        print("+", shlex.join(argv), flush=True)
    try:
        return subprocess.run(
            argv,
            cwd=cwd,
            check=check,
            text=True,
            stdout=subprocess.PIPE if capture else None,
            stderr=subprocess.PIPE if capture else None,
        )
    except FileNotFoundError as error:
        raise FixtureError(f"required command not found: {argv[0]}") from error
    except subprocess.CalledProcessError as error:
        detail = (error.stderr or "").strip()
        suffix = f": {detail}" if detail else ""
        raise FixtureError(
            f"command failed with status {error.returncode}: "
            f"{shlex.join(argv)}{suffix}"
        ) from error


def ffmpeg(
    context: BuildContext,
    *arguments: os.PathLike[str] | str,
    cwd: Path | None = None,
) -> None:
    run(
        [
            context.tools.ffmpeg,
            "-hide_banner",
            "-nostdin",
            "-loglevel",
            "error",
            "-y",
            *arguments,
        ],
        cwd=cwd,
    )


def tool_path(name: str) -> str:
    path = shutil.which(name)
    if path is None:
        raise FixtureError(f"required command not found: {name}")
    return path


def discover_tools(*, generation: bool) -> Tools:
    ffprobe = tool_path("ffprobe")
    ffmpeg_path = tool_path("ffmpeg") if generation else shutil.which("ffmpeg")
    return Tools(ffmpeg=ffmpeg_path or "ffmpeg", ffprobe=ffprobe)


def discover_font(explicit: str | None) -> Path:
    if explicit is not None:
        candidate = Path(explicit).expanduser().resolve()
        if not candidate.is_file():
            raise FixtureError(f"font does not exist: {candidate}")
        return candidate

    fc_match = shutil.which("fc-match")
    if fc_match is not None:
        result = run(
            [fc_match, "-f", "%{file}\n", "DejaVu Sans"], capture=True
        )
        first = result.stdout.splitlines()[0].strip() if result.stdout else ""
        if first and Path(first).is_file():
            return Path(first).resolve()

    for candidate in (
        Path("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
        Path("/usr/share/fonts/dejavu/DejaVuSans.ttf"),
        Path.home() / ".local/share/fonts/DejaVuSans.ttf",
    ):
        if candidate.is_file():
            return candidate.resolve()

    raise FixtureError(
        "could not find DejaVu Sans; install fontconfig/dejavu-fonts or pass "
        "--font /path/to/a redistributable .ttf"
    )


def require_ffmpeg_capabilities(tools: Tools) -> None:
    encoders = run(
        [tools.ffmpeg, "-hide_banner", "-encoders"], capture=True
    ).stdout
    required_encoders = (
        "libx264",
        "libx265",
        "libvpx-vp9",
        "libaom-av1",
        "aac",
        "ass",
    )
    missing = [name for name in required_encoders if name not in encoders]

    filters = run(
        [tools.ffmpeg, "-hide_banner", "-filters"], capture=True
    ).stdout
    required_filters = ("testsrc2", "sine", "setparams", "format")
    missing.extend(
        f"filter:{name}" for name in required_filters if name not in filters
    )

    if missing:
        raise FixtureError(
            "FFmpeg lacks required fixture capabilities: " + ", ".join(missing)
        )


def lavfi_inputs(*, second_audio: bool = False) -> list[str]:
    arguments = [
        "-f",
        "lavfi",
        "-i",
        f"testsrc2=size={VIDEO_SIZE}:rate={FRAME_RATE}:duration={DURATION_SECONDS}",
        "-f",
        "lavfi",
        "-i",
        f"sine=frequency=440:sample_rate=48000:duration={DURATION_SECONDS}",
    ]
    if second_audio:
        arguments.extend(
            [
                "-f",
                "lavfi",
                "-i",
                f"sine=frequency=660:sample_rate=48000:duration={DURATION_SECONDS}",
            ]
        )
    return arguments


def common_maps() -> list[str]:
    return ["-map", "0:v:0", "-map", "1:a:0"]


def common_audio() -> list[str]:
    return ["-c:a", "aac", "-b:a", "96k", "-af", "volume=0.08"]


def common_output(title: str) -> list[str]:
    return [
        "-t",
        str(DURATION_SECONDS),
        "-map_metadata",
        "-1",
        "-metadata",
        f"title={title}",
    ]


def build_codec_fixtures(context: BuildContext) -> None:
    root = context.root

    ffmpeg(
        context,
        *lavfi_inputs(),
        *common_maps(),
        "-vf",
        SDR_FILTER,
        "-c:v",
        "libx264",
        "-preset",
        "veryfast",
        "-crf",
        "23",
        "-g",
        str(FRAME_RATE),
        "-keyint_min",
        str(FRAME_RATE),
        "-sc_threshold",
        "0",
        "-threads",
        "2",
        *common_audio(),
        *common_output("Ferrex H.264 SDR 8-bit fixture"),
        root / "h264-sdr-8bit.mkv",
    )

    x265_common = (
        "log-level=error:pools=1:frame-threads=1:repeat-headers=1"
    )
    ffmpeg(
        context,
        *lavfi_inputs(),
        *common_maps(),
        "-vf",
        SDR_10_FILTER,
        "-c:v",
        "libx265",
        "-preset",
        "ultrafast",
        "-crf",
        "28",
        "-x265-params",
        f"{x265_common}:colorprim=1:transfer=1:colormatrix=1",
        *common_audio(),
        *common_output("Ferrex HEVC Main10 SDR fixture"),
        root / "hevc-main10-sdr.mkv",
    )

    pq_parameters = (
        f"{x265_common}:colorprim=9:transfer=16:colormatrix=9:hdr10=1:"
        "master-display=G(13250,34500)B(7500,3000)R(34000,16000)"
        "WP(15635,16450)L(10000000,50):max-cll=1000,400"
    )
    ffmpeg(
        context,
        *lavfi_inputs(),
        *common_maps(),
        "-vf",
        PQ_FILTER,
        "-c:v",
        "libx265",
        "-preset",
        "ultrafast",
        "-crf",
        "28",
        "-x265-params",
        pq_parameters,
        *common_audio(),
        *common_output("Ferrex synthetic HDR10 PQ metadata fixture"),
        root / "hdr10-pq.mkv",
    )

    ffmpeg(
        context,
        *lavfi_inputs(),
        *common_maps(),
        "-vf",
        HLG_FILTER,
        "-c:v",
        "libx265",
        "-preset",
        "ultrafast",
        "-crf",
        "28",
        "-x265-params",
        f"{x265_common}:colorprim=9:transfer=18:colormatrix=9",
        *common_audio(),
        *common_output("Ferrex synthetic HLG metadata fixture"),
        root / "hlg.mkv",
    )

    ffmpeg(
        context,
        *lavfi_inputs(),
        *common_maps(),
        "-vf",
        SDR_FILTER,
        "-c:v",
        "libvpx-vp9",
        "-deadline",
        "realtime",
        "-cpu-used",
        "8",
        "-crf",
        "35",
        "-b:v",
        "0",
        "-threads",
        "2",
        *common_audio(),
        *common_output("Ferrex VP9 SDR fixture"),
        root / "vp9-sdr.mkv",
    )

    ffmpeg(
        context,
        *lavfi_inputs(),
        *common_maps(),
        "-vf",
        SDR_FILTER,
        "-c:v",
        "libaom-av1",
        "-cpu-used",
        "8",
        "-crf",
        "40",
        "-b:v",
        "0",
        "-row-mt",
        "1",
        "-threads",
        "4",
        *common_audio(),
        *common_output("Ferrex AV1 SDR fixture"),
        root / "av1-sdr.mkv",
    )


def write_text_sources(context: BuildContext) -> None:
    context.sources.mkdir(parents=True, exist_ok=True)
    (context.sources / "animated.ass").write_text(
        "[Script Info]\n"
        "Title: Ferrex animated ASS fixture\n"
        "ScriptType: v4.00+\n"
        "PlayResX: 640\n"
        "PlayResY: 360\n"
        "ScaledBorderAndShadow: yes\n\n"
        "[V4+ Styles]\n"
        "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, "
        "OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, "
        "ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, "
        "Alignment, MarginL, MarginR, MarginV, Encoding\n"
        "Style: Default,DejaVu Sans,32,&H00FFFFFF,&H0000FFFF,&H00000000,"
        "&H80000000,0,0,0,0,100,100,0,0,1,2,1,2,20,20,24,1\n\n"
        "[Events]\n"
        "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, "
        "Effect, Text\n"
        "Dialogue: 0,0:00:00.40,0:00:03.60,Default,,0,0,0,,"
        "{\\move(80,300,560,300)\\bord3}Ferrex ASS motion\n"
        "Dialogue: 1,0:00:01.00,0:00:03.40,Default,,0,0,0,,"
        "{\\an8\\pos(320,48)\\k30}Ani{\\k30}ma{\\k30}ted\n",
        encoding="utf-8",
    )
    (context.sources / "english.srt").write_text(
        "1\n00:00:00,500 --> 00:00:02,000\nEnglish text subtitle\n\n"
        "2\n00:00:02,200 --> 00:00:03,700\nSecond English cue\n",
        encoding="utf-8",
    )
    (context.sources / "spanish.srt").write_text(
        "1\n00:00:00,700 --> 00:00:02,200\nSubtítulo en español\n\n"
        "2\n00:00:02,300 --> 00:00:03,800\nSegunda pista\n",
        encoding="utf-8",
    )
    (context.sources / "chapters.ffmeta").write_text(
        ";FFMETADATA1\n"
        "title=Ferrex chapter fixture\n"
        "[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=2000\n"
        "title=Opening\n"
        "[CHAPTER]\nTIMEBASE=1/1000\nSTART=2000\nEND=4000\n"
        "title=Second chapter\n",
        encoding="utf-8",
    )


def build_ass_fixture(context: BuildContext) -> None:
    ffmpeg(
        context,
        *lavfi_inputs(),
        "-i",
        context.sources / "animated.ass",
        *common_maps(),
        "-map",
        "2:s:0",
        "-vf",
        SDR_FILTER,
        "-c:v",
        "libx264",
        "-preset",
        "veryfast",
        "-crf",
        "23",
        "-threads",
        "2",
        *common_audio(),
        "-c:s",
        "ass",
        "-metadata:s:s:0",
        "language=eng",
        "-metadata:s:s:0",
        "title=Animated ASS",
        "-disposition:s:0",
        "default",
        "-attach",
        context.font,
        "-metadata:s:t:0",
        "mimetype=application/x-truetype-font",
        "-metadata:s:t:0",
        "filename=DejaVuSans.ttf",
        *common_output("Ferrex ASS animation and font fixture"),
        context.root / "ass-animation-fonts.mkv",
    )


def write_pgs(path: Path) -> None:
    """Write a tiny valid SUP stream without relying on a PGS encoder."""

    width = 160
    height = 40

    with path.open("wb") as output:
        def segment(pts_seconds: float, kind: int, payload: bytes) -> None:
            timestamp = int(pts_seconds * 90_000)
            output.write(b"PG")
            output.write(
                struct.pack(">IIBH", timestamp, timestamp, kind, len(payload))
            )
            output.write(payload)

        # Epoch-start composition with one object in one window.
        segment(
            1.0,
            0x16,
            struct.pack(
                ">HHBHB BBBHBBHH",
                640,
                360,
                0x10,
                0,
                0x80,
                0,
                0,
                1,
                0,
                0,
                0,
                240,
                280,
            ),
        )
        segment(
            1.0,
            0x17,
            struct.pack(">BBHHHH", 1, 0, 240, 280, width, height),
        )
        # Transparent, white, and black entries in Y/Cr/Cb/alpha order.
        segment(
            1.0,
            0x14,
            bytes(
                [
                    0,
                    0,
                    0,
                    16,
                    128,
                    128,
                    0,
                    1,
                    235,
                    128,
                    128,
                    255,
                    2,
                    16,
                    128,
                    128,
                    255,
                ]
            ),
        )

        # PGS permits a nonzero palette index to represent one literal pixel.
        # This intentionally uncompressed bordered rectangle stays small and
        # makes the generator independent of third-party subtitle encoders.
        rle = bytearray()
        for y in range(height):
            for x in range(width):
                border = y in (0, height - 1) or x in (0, width - 1)
                rle.append(1 if border else 2)
            rle.extend((0, 0))  # end of line

        object_data_length = 4 + len(rle)
        object_payload = (
            struct.pack(">HBB", 0, 0, 0xC0)
            + object_data_length.to_bytes(3, "big")
            + struct.pack(">HH", width, height)
            + rle
        )
        segment(1.0, 0x15, object_payload)
        segment(1.0, 0x80, b"")

        # Clear the composition near the end of the fixture.
        segment(
            3.0,
            0x16,
            struct.pack(">HHBHB BBB", 640, 360, 0x10, 1, 0, 0, 0, 0),
        )
        segment(3.0, 0x80, b"")


def build_pgs_fixture(context: BuildContext) -> None:
    sup = context.sources / "bitmap.sup"
    write_pgs(sup)
    ffmpeg(
        context,
        *lavfi_inputs(),
        "-f",
        "sup",
        "-i",
        sup,
        *common_maps(),
        "-map",
        "2:s:0",
        "-vf",
        SDR_FILTER,
        "-c:v",
        "libx264",
        "-preset",
        "veryfast",
        "-crf",
        "23",
        "-threads",
        "2",
        *common_audio(),
        "-c:s",
        "copy",
        "-metadata:s:s:0",
        "language=eng",
        "-metadata:s:s:0",
        "title=Synthetic PGS bitmap",
        *common_output("Ferrex PGS bitmap subtitle fixture"),
        context.root / "pgs-bitmap.mkv",
    )


def build_multitrack_fixture(context: BuildContext) -> None:
    ffmpeg(
        context,
        *lavfi_inputs(second_audio=True),
        "-i",
        context.sources / "english.srt",
        "-i",
        context.sources / "spanish.srt",
        "-f",
        "ffmetadata",
        "-i",
        context.sources / "chapters.ffmeta",
        "-map",
        "0:v:0",
        "-map",
        "1:a:0",
        "-map",
        "2:a:0",
        "-map",
        "3:s:0",
        "-map",
        "4:s:0",
        "-map_metadata",
        "5",
        "-map_chapters",
        "5",
        "-vf",
        SDR_FILTER,
        "-c:v",
        "libx264",
        "-preset",
        "veryfast",
        "-crf",
        "23",
        "-threads",
        "2",
        "-c:a",
        "aac",
        "-b:a",
        "96k",
        "-filter:a:0",
        "volume=0.08",
        "-filter:a:1",
        "volume=0.08",
        "-metadata:s:a:0",
        "language=eng",
        "-metadata:s:a:0",
        "title=English stereo",
        "-metadata:s:a:1",
        "language=spa",
        "-metadata:s:a:1",
        "title=Spanish stereo",
        "-disposition:a:0",
        "default",
        "-disposition:a:1",
        "0",
        "-c:s",
        "srt",
        "-metadata:s:s:0",
        "language=eng",
        "-metadata:s:s:0",
        "title=English text",
        "-metadata:s:s:1",
        "language=spa",
        "-metadata:s:s:1",
        "title=Spanish forced text",
        "-disposition:s:0",
        "default",
        "-disposition:s:1",
        "forced",
        "-attach",
        context.font,
        "-metadata:s:t:0",
        "mimetype=application/x-truetype-font",
        "-metadata:s:t:0",
        "filename=DejaVuSans.ttf",
        "-t",
        str(DURATION_SECONDS),
        context.root / "multitrack-structure.mkv",
    )


def build_hls_fixture(context: BuildContext) -> None:
    hls = context.root / "transcoded-hls"
    hls.mkdir()
    ffmpeg(
        context,
        "-i",
        context.root / "h264-sdr-8bit.mkv",
        "-map",
        "0:v:0",
        "-map",
        "0:a:0",
        "-c",
        "copy",
        "-f",
        "hls",
        "-hls_time",
        "1",
        "-hls_playlist_type",
        "vod",
        "-hls_flags",
        "independent_segments",
        "-hls_segment_filename",
        "segment-%03d.ts",
        "index.m3u8",
        cwd=hls,
    )


def build_malformed_fixtures(context: BuildContext) -> None:
    valid = (context.root / "h264-sdr-8bit.mkv").read_bytes()
    # Keep a recognizable EBML prefix but remove the segment body.
    (context.root / "malformed-truncated.mkv").write_bytes(valid[:64])
    (context.root / "unsupported.txt").write_text(
        "This is deliberately not a media container.\n", encoding="utf-8"
    )


def ffmpeg_version(tools: Tools) -> str:
    first = run([tools.ffmpeg, "-version"], capture=True).stdout.splitlines()
    return first[0] if first else "unknown"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_manifest(context: BuildContext) -> None:
    manifest = {
        "schema_version": SCHEMA_VERSION,
        "generator": "scripts/qa/native_playback_fixtures.py",
        "ffmpeg": ffmpeg_version(context.tools),
        "duration_seconds": DURATION_SECONDS,
        "video_size": VIDEO_SIZE,
        "frame_rate": FRAME_RATE,
        "font": {
            "embedded_filename": "DejaVuSans.ttf",
            "sha256": sha256(context.font),
        },
        "fixtures": [
            {
                "path": "h264-sdr-8bit.mkv",
                "purpose": "H.264 SDR 8-bit baseline",
            },
            {
                "path": "hevc-main10-sdr.mkv",
                "purpose": "HEVC Main10 SDR decode and output-color separation",
            },
            {
                "path": "hdr10-pq.mkv",
                "purpose": "BT.2020/PQ mastering-display and MaxCLL metadata",
            },
            {
                "path": "hlg.mkv",
                "purpose": "BT.2020/ARIB STD-B67 HLG signaling",
            },
            {"path": "vp9-sdr.mkv", "purpose": "VP9 SDR decode"},
            {"path": "av1-sdr.mkv", "purpose": "AV1 SDR decode"},
            {
                "path": "ass-animation-fonts.mkv",
                "purpose": "animated ASS plus attached font",
            },
            {
                "path": "pgs-bitmap.mkv",
                "purpose": "HDMV PGS bitmap subtitle",
            },
            {
                "path": "multitrack-structure.mkv",
                "purpose": "multiple audio/subtitle tracks, chapters, and attachment",
            },
            {
                "path": "transcoded-hls/index.m3u8",
                "purpose": "local server-style HLS/transcode output",
            },
            {
                "path": "malformed-truncated.mkv",
                "purpose": "truncated-container failure",
            },
            {
                "path": "unsupported.txt",
                "purpose": "unsupported-input failure",
            },
        ],
    }
    (context.root / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    (context.root / "README.txt").write_text(
        "Ferrex native playback fixtures\n"
        "===============================\n\n"
        "Generated locally by scripts/qa/native_playback_fixtures.py.\n"
        "Do not commit this directory. Verify it with:\n\n"
        "  ./scripts/qa/native_playback_fixtures.py verify\n\n"
        "The HDR10 and HLG files validate signaling and playback behavior;\n"
        "their synthetic test pattern is not a visual mastering reference.\n",
        encoding="utf-8",
    )


def checksum_files(root: Path) -> Iterable[Path]:
    excluded = {MARKER_NAME, "SHA256SUMS"}
    return (
        path
        for path in sorted(root.rglob("*"))
        if path.is_file() and path.name not in excluded
    )


def write_checksums(root: Path) -> None:
    lines = [
        f"{sha256(path)}  {path.relative_to(root).as_posix()}"
        for path in checksum_files(root)
    ]
    (root / "SHA256SUMS").write_text("\n".join(lines) + "\n", encoding="utf-8")


def probe_json(
    tools: Tools,
    path: Path,
    *,
    frames: bool = False,
) -> dict[str, Any]:
    arguments = [tools.ffprobe, "-v", "error"]
    if frames:
        arguments.extend(
            ["-select_streams", "v:0", "-read_intervals", "%+#1", "-show_frames"]
        )
    else:
        arguments.extend(["-show_streams", "-show_chapters", "-show_format"])
    arguments.extend(["-of", "json", path])
    result = run(arguments, capture=True)
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise FixtureError(f"ffprobe returned invalid JSON for {path}") from error


def streams(probe: dict[str, Any], kind: str) -> list[dict[str, Any]]:
    return [
        stream
        for stream in probe.get("streams", [])
        if stream.get("codec_type") == kind
    ]


def packet_counts(tools: Tools, path: Path, selector: str) -> list[int]:
    result = run(
        [
            tools.ffprobe,
            "-v",
            "error",
            "-select_streams",
            selector,
            "-count_packets",
            "-show_entries",
            "stream=nb_read_packets",
            "-of",
            "json",
            path,
        ],
        capture=True,
    )
    try:
        probe = json.loads(result.stdout)
        return [
            int(stream["nb_read_packets"])
            for stream in probe.get("streams", [])
            if stream.get("nb_read_packets") not in (None, "N/A")
        ]
    except (KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise FixtureError(f"could not count packets in {path}") from error


def require(condition: bool, message: str) -> None:
    if not condition:
        raise FixtureError(message)


def verify_video(
    tools: Tools,
    root: Path,
    filename: str,
    codec: str,
    *,
    pixel_format: str | None = None,
    transfer: str | None = None,
    primaries: str | None = None,
) -> dict[str, Any]:
    probe = probe_json(tools, root / filename)
    video = streams(probe, "video")
    audio = streams(probe, "audio")
    require(len(video) == 1, f"{filename}: expected one video stream")
    require(len(audio) >= 1, f"{filename}: expected an audio stream")
    stream = video[0]
    require(
        stream.get("codec_name") == codec,
        f"{filename}: expected {codec}, got {stream.get('codec_name')}",
    )
    if pixel_format is not None:
        require(
            stream.get("pix_fmt") == pixel_format,
            f"{filename}: expected pixel format {pixel_format}, "
            f"got {stream.get('pix_fmt')}",
        )
    if transfer is not None:
        require(
            stream.get("color_transfer") == transfer,
            f"{filename}: expected transfer {transfer}, "
            f"got {stream.get('color_transfer')}",
        )
    if primaries is not None:
        require(
            stream.get("color_primaries") == primaries,
            f"{filename}: expected primaries {primaries}, "
            f"got {stream.get('color_primaries')}",
        )
    return probe


def verify_checksums(root: Path) -> None:
    checksum_path = root / "SHA256SUMS"
    require(checksum_path.is_file(), "SHA256SUMS is missing")
    for line in checksum_path.read_text(encoding="utf-8").splitlines():
        expected, separator, relative = line.partition("  ")
        require(bool(separator and relative), f"invalid checksum line: {line}")
        path = root / relative
        require(path.is_file(), f"checksummed fixture is missing: {relative}")
        require(
            sha256(path) == expected,
            f"fixture checksum mismatch: {relative}",
        )


def verify_directory(root: Path, tools: Tools) -> None:
    root = root.resolve()
    require(root.is_dir(), f"fixture directory does not exist: {root}")
    marker = root / MARKER_NAME
    require(marker.is_file(), f"fixture marker is missing: {marker}")
    require(
        marker.read_text(encoding="utf-8").strip() == str(SCHEMA_VERSION),
        "fixture marker schema is incompatible",
    )

    manifest_path = root / "manifest.json"
    require(manifest_path.is_file(), "manifest.json is missing")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    require(
        manifest.get("schema_version") == SCHEMA_VERSION,
        "manifest schema is incompatible",
    )

    for filename in PRIMARY_FIXTURES:
        require((root / filename).is_file(), f"fixture is missing: {filename}")

    verify_video(
        tools,
        root,
        "h264-sdr-8bit.mkv",
        "h264",
        pixel_format="yuv420p",
        transfer="bt709",
        primaries="bt709",
    )
    verify_video(
        tools,
        root,
        "hevc-main10-sdr.mkv",
        "hevc",
        pixel_format="yuv420p10le",
        transfer="bt709",
        primaries="bt709",
    )
    verify_video(
        tools,
        root,
        "hdr10-pq.mkv",
        "hevc",
        pixel_format="yuv420p10le",
        transfer="smpte2084",
        primaries="bt2020",
    )
    verify_video(
        tools,
        root,
        "hlg.mkv",
        "hevc",
        pixel_format="yuv420p10le",
        transfer="arib-std-b67",
        primaries="bt2020",
    )
    verify_video(tools, root, "vp9-sdr.mkv", "vp9")
    verify_video(tools, root, "av1-sdr.mkv", "av1")

    pq_frames = probe_json(tools, root / "hdr10-pq.mkv", frames=True)
    side_data_types = {
        entry.get("side_data_type")
        for frame in pq_frames.get("frames", [])
        for entry in frame.get("side_data_list", [])
    }
    require(
        "Mastering display metadata" in side_data_types,
        "hdr10-pq.mkv: mastering-display metadata is missing",
    )
    require(
        "Content light level metadata" in side_data_types,
        "hdr10-pq.mkv: MaxCLL/MaxFALL metadata is missing",
    )

    ass_probe = verify_video(tools, root, "ass-animation-fonts.mkv", "h264")
    require(
        any(stream.get("codec_name") == "ass" for stream in streams(ass_probe, "subtitle")),
        "ass-animation-fonts.mkv: ASS stream is missing",
    )
    require(
        len(streams(ass_probe, "attachment")) >= 1,
        "ass-animation-fonts.mkv: attached font is missing",
    )
    require(
        packet_counts(tools, root / "ass-animation-fonts.mkv", "s") == [2],
        "ass-animation-fonts.mkv: expected two animated subtitle events",
    )

    pgs_probe = verify_video(tools, root, "pgs-bitmap.mkv", "h264")
    require(
        any(
            stream.get("codec_name") == "hdmv_pgs_subtitle"
            for stream in streams(pgs_probe, "subtitle")
        ),
        "pgs-bitmap.mkv: PGS stream is missing",
    )
    require(
        packet_counts(tools, root / "pgs-bitmap.mkv", "s") == [2],
        "pgs-bitmap.mkv: expected show and clear display sets",
    )

    structure = verify_video(
        tools, root, "multitrack-structure.mkv", "h264"
    )
    require(
        len(streams(structure, "audio")) >= 2,
        "multitrack-structure.mkv: expected two audio tracks",
    )
    require(
        len(streams(structure, "subtitle")) >= 2,
        "multitrack-structure.mkv: expected two subtitle tracks",
    )
    require(
        len(streams(structure, "attachment")) >= 1,
        "multitrack-structure.mkv: expected an attachment",
    )
    require(
        len(structure.get("chapters", [])) >= 2,
        "multitrack-structure.mkv: expected two chapters",
    )

    playlist = root / "transcoded-hls/index.m3u8"
    require(playlist.is_file(), "transcoded HLS playlist is missing")
    playlist_text = playlist.read_text(encoding="utf-8")
    require("#EXT-X-ENDLIST" in playlist_text, "HLS VOD playlist is incomplete")
    require(
        "segment-" in playlist_text and list(playlist.parent.glob("segment-*.ts")),
        "HLS transport segments are missing",
    )
    hls_probe = probe_json(tools, playlist)
    require(streams(hls_probe, "video"), "HLS fixture has no video stream")
    require(streams(hls_probe, "audio"), "HLS fixture has no audio stream")

    for malformed in ("malformed-truncated.mkv", "unsupported.txt"):
        result = subprocess.run(
            [tools.ffprobe, "-v", "error", root / malformed],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        require(
            result.returncode != 0,
            f"{malformed}: expected ffprobe to reject malformed input",
        )

    verify_checksums(root)
    print(f"Verified {len(PRIMARY_FIXTURES)} primary playback fixtures in {root}")


def generate(output: Path, *, force: bool, font: str | None) -> None:
    output = output.expanduser().resolve()
    parent = output.parent
    parent.mkdir(parents=True, exist_ok=True)

    if output.exists():
        marker = output / MARKER_NAME
        if not force:
            raise FixtureError(
                f"output already exists: {output} (pass --force to replace it)"
            )
        if not marker.is_file():
            raise FixtureError(
                f"refusing to replace unmarked directory: {output}"
            )

    tools = discover_tools(generation=True)
    require_ffmpeg_capabilities(tools)
    selected_font = discover_font(font)

    temporary = Path(
        tempfile.mkdtemp(prefix=f".{output.name}.tmp-", dir=parent)
    ).resolve()
    context = BuildContext(root=temporary, tools=tools, font=selected_font)

    try:
        (temporary / MARKER_NAME).write_text(
            f"{SCHEMA_VERSION}\n", encoding="utf-8"
        )
        write_text_sources(context)
        build_codec_fixtures(context)
        build_ass_fixture(context)
        build_pgs_fixture(context)
        build_multitrack_fixture(context)
        build_hls_fixture(context)
        build_malformed_fixtures(context)
        write_manifest(context)
        write_checksums(temporary)
        verify_directory(temporary, tools)

        if output.exists():
            shutil.rmtree(output)
        temporary.replace(output)
    except BaseException:
        shutil.rmtree(temporary, ignore_errors=True)
        raise

    print(f"Generated playback fixtures: {output}")


def list_fixtures() -> None:
    for fixture in PRIMARY_FIXTURES:
        print(fixture)
    print("transcoded-hls/index.m3u8")
    print("malformed-truncated.mkv")
    print("unsupported.txt")


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    generate_parser = subparsers.add_parser(
        "generate", help="generate and verify the complete fixture set"
    )
    generate_parser.add_argument(
        "--output", "-o", type=Path, default=DEFAULT_OUTPUT
    )
    generate_parser.add_argument(
        "--force", action="store_true", help="replace a marked fixture directory"
    )
    generate_parser.add_argument(
        "--font",
        help="redistributable TrueType font to embed (defaults to DejaVu Sans)",
    )

    verify_parser = subparsers.add_parser(
        "verify", help="validate streams, metadata, structure, and checksums"
    )
    verify_parser.add_argument(
        "--output", "-o", type=Path, default=DEFAULT_OUTPUT
    )

    subparsers.add_parser("list", help="print the generated fixture paths")
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv if argv is not None else sys.argv[1:])
    try:
        if args.command == "generate":
            generate(args.output, force=args.force, font=args.font)
        elif args.command == "verify":
            verify_directory(
                args.output.expanduser().resolve(),
                discover_tools(generation=False),
            )
        else:
            list_fixtures()
    except FixtureError as error:
        print(f"native playback fixture error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
