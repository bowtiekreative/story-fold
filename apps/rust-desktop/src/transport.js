// Minimal WebView transport: Rust supplies the camera, source times, crop and play intent.
const command = await dioxus.recv();
const sound = document.getElementById('master-audio');
const slots = [document.getElementById('camera-0'), document.getElementById('camera-1')];
if (!sound || slots.some(v => !v)) return '';
const state = window.__storyfoldMedia || (window.__storyfoldMedia = {slot: 0, key: '', serial: 0});
state.command = command;
const serial = ++state.serial;
if (state.key !== command.key) { state.slot = 1 - state.slot; state.key = command.key; }
const active = slots[state.slot], spare = slots[1 - state.slot];
function source(element, url) {
    if (element.getAttribute('src') !== url) { element.src = url; element.load(); }
}
function seek(element, time, tolerance) {
    if (element.readyState && Number.isFinite(time) && Math.abs(element.currentTime - time) > tolerance) {
        try { element.currentTime = Math.max(0, time); } catch (_) { /* metadata event retries */ }
    }
}
source(sound, command.audio);
source(active, command.video);
active.style.opacity = '1'; spare.style.opacity = '0';
active.style.transform = command.transform;
spare.pause();
if (command.next_video) { source(spare, command.next_video); seek(spare, command.next_time, 0.1); }
seek(active, command.video_time, 0.16);
seek(sound, command.audio_time, 0.12);
// Metadata can arrive after the first paused preview command.
active.onloadedmetadata = () => {
    const c = state.command;
    if (slots[state.slot] === active) seek(active, c.video_time, 0.02);
};
sound.onloadedmetadata = () => seek(sound, state.command.audio_time, 0.02);
if (!command.playing) { sound.pause(); active.pause(); return ''; }
try {
    // Both promises are observed, so decoder/autoplay failures never leave a false playing state.
    await Promise.all([sound.play(), active.play()]);
    if (state.serial !== serial && !state.command.playing) { sound.pause(); active.pause(); }
    return '';
} catch (_) {
    if (state.serial !== serial) return '';
    sound.pause(); active.pause();
    return 'Preview could not start. Press Play to retry, or check that the media service is running.';
}
