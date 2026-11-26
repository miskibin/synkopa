(() => {
  const example = document.querySelector("[data-delay-audition]");
  if (!example) return;

  const dry = example.querySelector("[data-dry]");
  const wet = example.querySelector("[data-wet]");
  const stop = example.querySelector("[data-stop]");
  const status = example.querySelector("[role=status]");
  const playback = new Audio();
  playback.preload = "none";
  let request = 0;

  function reset(message = "") {
    request++;
    playback.pause();
    playback.removeAttribute("src");
    playback.load();
    stop.disabled = true;
    status.textContent = message;
  }

  async function play(file, label) {
    const current = ++request;
    playback.src = "audio/" + file;
    stop.disabled = false;
    status.textContent = "Loading audio…";
    try {
      await playback.play();
      if (current === request) status.textContent = label;
    } catch {
      if (current === request) reset("Could not play the audio example.");
    }
  }

  dry.addEventListener("click", () => play("delay-dry.mp3", "Playing dry"));
  wet.addEventListener("click", () => play("delay-wet.mp3", "Playing with delay"));
  stop.addEventListener("click", () => reset("Stopped"));
  playback.addEventListener("ended", () => reset("Finished"));
  document.addEventListener("visibilitychange", () => {
    if (document.hidden && !stop.disabled) reset();
  });
  window.addEventListener("pagehide", () => {
    if (!stop.disabled) reset();
  });
  example.hidden = false;
})();
