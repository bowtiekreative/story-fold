import { useEffect, useRef, useState } from "react";
import {
  frameAt,
  sequence,
  type Project,
  type Sentence,
} from "../../../packages/timeline/index.ts";
import { Icon, Badge } from "./ui.tsx";
export function Player({
  project,
  selected,
  mode,
  time,
  playing,
  onTime,
  onPlaying,
}: {
  project: Project;
  selected: Sentence | undefined;
  mode: "story" | "sentence";
  time: number;
  playing: boolean;
  onTime: (t: number) => void;
  onPlaying: (p: boolean) => void;
}) {
  const sq = sequence(project),
    frame = frameAt(sq, time);
  const sentence = mode === "story" ? frame.video?.sentence : selected;
  const audioSentence = mode === "story" ? frame.audio?.sentence : selected;
  const sourceTime =
    mode === "story" ? frame.videoTime : (selected?.in ?? 0) + time;
  const audioTime =
    mode === "story" ? frame.audioTime : (selected?.in ?? 0) + time;
  const camera =
    project.cameras.find((c) => c.id === sentence?.cam) ?? project.cameras[0];
  const visible = useRef<HTMLVideoElement>(null),
    spare = useRef<HTMLVideoElement>(null),
    audio = useRef<HTMLAudioElement>(null);
  const [slot, setSlot] = useState(0);
  const clipKey = useRef("");
  const latest = useRef({
    time,
    playing,
    mode,
    selected,
    sq,
    onTime,
    onPlaying,
  });
  latest.current = { time, playing, mode, selected, sq, onTime, onPlaying };
  useEffect(() => {
    let previous = performance.now(),
      raf = 0;
    const tick = (now: number) => {
      const s = latest.current;
      const dt = Math.min(0.1, (now - previous) / 1000);
      previous = now;
      const length =
        s.mode === "story"
          ? s.sq.total
          : s.selected
            ? s.selected.out - s.selected.in
            : 0;
      if (
        s.playing &&
        audio.current &&
        audio.current.readyState >= 2 &&
        !audio.current.seeking
      ) {
        const next = s.time + dt;
        if (next >= length) {
          s.onTime(length);
          s.onPlaying(false);
        } else s.onTime(next);
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, []);
  useEffect(() => {
    const key =
      mode === "story"
        ? `${frame.video?.itemIndex}:${frame.video?.vStart}:${camera.id}`
        : `sel:${selected?.id}:${camera.id}`;
    let active = slot;
    if (key !== clipKey.current) {
      active = 1 - slot;
      clipKey.current = key;
      setSlot(active);
    }
    const video = active === 0 ? visible.current : spare.current,
      other = active === 0 ? spare.current : visible.current;
    const src = `/api/projects/${project.id}/media/${camera.id}`;
    if (video) {
      if (video.getAttribute("src") !== src) video.src = src;
      if (
        Number.isFinite(sourceTime) &&
        Math.abs(video.currentTime - (sourceTime + camera.offset)) > 0.16
      )
        video.currentTime = Math.max(0, sourceTime + camera.offset);
      if (playing) void video.play().catch(() => {});
      else video.pause();
    }
    if (other) {
      other.pause();
      const idx = sq.clips.indexOf(frame.video!);
      const next = sq.clips[idx + 1];
      if (mode === "story" && next) {
        const nextCam = project.cameras.find(
          (c) => c.id === next.sentence.cam,
        )!;
        const nextSrc = `/api/projects/${project.id}/media/${nextCam.id}`;
        if (other.getAttribute("src") !== nextSrc) other.src = nextSrc;
        if (
          Math.abs(other.currentTime - next.sentence.in - nextCam.offset) > 0.1
        )
          other.currentTime = Math.max(0, next.sentence.in + nextCam.offset);
      }
    }
    const sound = audio.current;
    if (sound && audioSentence) {
      if (Math.abs(sound.currentTime - audioTime) > 0.12)
        sound.currentTime = Math.max(0, audioTime);
      if (playing) void sound.play().catch(() => onPlaying(false));
      else sound.pause();
    }
  }, [sourceTime, audioTime, playing, camera.id, mode, selected?.id]);
  const crop = camera.virtual;
  const transform = crop
    ? `scale(${crop.scale}) translate(${(0.5 - crop.x) * 40}%,${(0.5 - crop.y) * 40}%)`
    : undefined;
  return (
    <div className={"video-view " + (!sentence ? "video-empty" : "")}>
      <video
        ref={visible}
        muted
        playsInline
        preload="auto"
        style={{ opacity: slot === 0 && sentence ? 1 : 0, transform }}
      />
      <video
        ref={spare}
        muted
        playsInline
        preload="auto"
        style={{ opacity: slot === 1 && sentence ? 1 : 0, transform }}
      />
      <audio
        ref={audio}
        src={`/api/projects/${project.id}/media/A`}
        preload="auto"
      />
      {sentence ? (
        <>
          <div className="viewer-top">
            <Badge tone="dark">
              CAM {camera.id} · {camera.name}
            </Badge>
            <span className="mono">
              {project.sample ? "SYNTHETIC SAMPLE" : "SOURCE FOOTAGE"}
            </span>
          </div>
          {project.exportSettings.captions && (
            <div className="caption-preview">{audioSentence?.text}</div>
          )}
          {mode === "story" && frame.lcut && (
            <span className="lcut-badge">L-cut · audio carries over</span>
          )}
        </>
      ) : (
        <div className="viewer-empty-content">
          <Icon name="film" size={32} />
          <strong>A story starts with a sentence.</strong>
          <span>Select one on the right, or add a folder below.</span>
        </div>
      )}
    </div>
  );
}
