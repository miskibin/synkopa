(() => {
  const demo = document.querySelector("[data-envelope-demo]");
  const AudioEngine = window.AudioContext || window.webkitAudioContext;
  if (!demo || !AudioEngine) return;

  const notes = {
    pluck: { attack: 0.008, decay: 0.25, sustain: 0, release: 0.10 },
    swell: { attack: 0.45, decay: 0.25, sustain: 0.65, release: 0.65 },
  };
  const gate = 0.8;
  const meter = demo.querySelector("progress");
  const stop = demo.querySelector("[data-envelope-stop]");
  const status = demo.querySelector("[role=status]");
  let context;
  let voice;
  let generation = 0;
  let frame;

  // Shape the stages like the synth's exponential ADSR, with a single fixed-pitch oscillator.
  const ramp = (position, ratio) => -Math.expm1(-Math.log1p(1 / ratio) * position) * (1 + ratio);
  function levelAt(elapsed, { attack, decay, sustain, release }) {
    if (elapsed <= 0) return 0;
    if (elapsed < attack) return ramp(elapsed / attack, 0.3);
    if (elapsed < attack + decay) return 1 - (1 - sustain) * ramp((elapsed - attack) / decay, 0.0001);
    if (elapsed < gate) return sustain;
    if (elapsed < gate + release) return sustain * (1 - ramp((elapsed - gate) / release, 0.0001));
    return 0;
  }

  function reset(message = "") {
    generation++;
    cancelAnimationFrame(frame);
    if (voice) {
      const now = context.currentTime;
      voice.output.gain.setValueAtTime(0.16, now);
      voice.output.gain.linearRampToValueAtTime(0, now + 0.012);
      voice.oscillator.stop(now + 0.012);
      voice = null;
    }
    meter.value = 0;
    stop.disabled = true;
    status.textContent = message;
  }

  async function play(kind) {
    reset();
    const current = generation;
    stop.disabled = false;
    status.textContent = "Preparing note…";
    try {
      context ??= new AudioEngine();
      if (context.state !== "running") await context.resume();
      if (current !== generation) return;

      const settings = notes[kind];
      const end = gate + settings.release;
      const start = context.currentTime + 0.025;
      const curve = Float32Array.from({ length: 2048 }, (_, n) => levelAt(end * n / 2047, settings));
      const oscillator = context.createOscillator();
      const envelope = context.createGain();
      const output = context.createGain();
      oscillator.type = "triangle";
      oscillator.frequency.value = 261.626;
      envelope.gain.value = 0;
      envelope.gain.setValueCurveAtTime(curve, start, end);
      output.gain.value = 0.16;
      oscillator.connect(envelope).connect(output).connect(context.destination);
      voice = { oscillator, output };
      oscillator.onended = () => {
        oscillator.disconnect();
        envelope.disconnect();
        output.disconnect();
        if (current !== generation) return;
        voice = null;
        reset("Note finished.");
      };
      oscillator.start(start);
      oscillator.stop(start + end);
      status.textContent = kind === "pluck" ? "Playing pluck." : "Playing swell.";

      function display() {
        meter.value = levelAt(context.currentTime - start, settings);
        frame = requestAnimationFrame(display);
      }
      frame = requestAnimationFrame(display);
    } catch {
      if (current === generation) reset("Could not play the note.");
    }
  }

  for (const button of demo.querySelectorAll("[data-envelope-note]")) {
    button.addEventListener("click", () => play(button.dataset.envelopeNote));
  }
  stop.addEventListener("click", () => reset("Stopped."));
  document.addEventListener("visibilitychange", () => {
    if (document.hidden && !stop.disabled) reset();
  });
  window.addEventListener("pagehide", () => {
    if (!stop.disabled) reset();
  });
  demo.hidden = false;
})();
