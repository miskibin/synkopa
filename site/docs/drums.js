const example = document.querySelector("[data-drums]");
const Audio = window.AudioContext || window.webkitAudioContext;

if (Audio) {
  const rows = [
    { name: "Kick", sample: "kick-808-short", steps: [1, 5, 9, 13] },
    { name: "Clap", sample: "clap-808", steps: [5, 13] },
    { name: "Hat", sample: "hat-808-closed", steps: [1, 3, 5, 7, 9, 11, 13, 15] },
  ];
  const grid = example.querySelector(".steps");
  const play = example.querySelector("[data-play]");
  const reset = example.querySelector("[data-reset]");
  const status = example.querySelector("[role=status]");
  const interval = 60 / 120 / 4;
  let context;
  let output;
  let samples;
  let timer;
  let startTime;
  let nextStep;
  let frame;
  let sounding = -1;
  const scheduled = new Set();

  for (const row of rows) {
    const label = document.createElement("span");
    label.textContent = row.name;
    grid.append(label);
    row.buttons = Array.from({ length: 16 }, (_, index) => {
      const step = document.createElement("button");
      step.type = "button";
      step.className = "step";
      if (index % 4 === 0) step.classList.add("beat");
      step.setAttribute("aria-label", row.name + " step " + (index + 1));
      step.setAttribute("aria-pressed", row.steps.includes(index + 1));
      step.addEventListener("click", () => {
        step.setAttribute("aria-pressed", step.getAttribute("aria-pressed") !== "true");
      });
      grid.append(step);
      return step;
    });
  }

  function highlight(step) {
    if (step === sounding) return;
    for (const row of rows) {
      row.buttons[sounding]?.classList.remove("playing");
      row.buttons[step]?.classList.add("playing");
    }
    sounding = step;
  }

  function schedule() {
    while (startTime + nextStep * interval < context.currentTime + .12) {
      const at = startTime + nextStep * interval;
      const step = nextStep % 16;
      nextStep += 1;
      if (at < context.currentTime) continue;
      for (const [index, row] of rows.entries()) {
        if (row.buttons[step].getAttribute("aria-pressed") !== "true") continue;
        const source = context.createBufferSource();
        source.buffer = samples[index];
        source.connect(output);
        source.onended = () => {
          scheduled.delete(source);
          source.disconnect();
        };
        scheduled.add(source);
        source.start(at);
      }
    }
  }

  function animate() {
    const index = Math.floor((context.currentTime - startTime) / interval);
    highlight(index < 0 ? -1 : index % 16);
    frame = requestAnimationFrame(animate);
  }

  function stop() {
    clearInterval(timer);
    timer = undefined;
    cancelAnimationFrame(frame);
    for (const source of scheduled) source.stop();
    scheduled.clear();
    highlight(-1);
    play.textContent = "Play loop";
  }

  play.addEventListener("click", async () => {
    if (timer !== undefined) {
      stop();
      return;
    }
    play.disabled = true;
    status.textContent = "";
    try {
      if (!context) {
        context = new Audio();
        output = context.createGain();
        output.gain.value = .32;
        output.connect(context.destination);
      }
      await context.resume();
      samples ??= await Promise.all(rows.map(async row => {
        const response = await fetch("audio/" + row.sample + ".mp3");
        if (!response.ok) throw new Error("Could not load drum sample");
        return context.decodeAudioData(await response.arrayBuffer());
      }));
      if (!document.hidden) {
        startTime = context.currentTime + .025;
        nextStep = 0;
        schedule();
        timer = setInterval(schedule, 30);
        animate();
        play.textContent = "Stop loop";
      }
    } catch {
      stop();
      status.textContent = "Could not play audio.";
    } finally {
      play.disabled = false;
    }
  });

  reset.addEventListener("click", () => {
    for (const row of rows) {
      row.buttons.forEach((step, index) => step.setAttribute("aria-pressed", row.steps.includes(index + 1)));
    }
  });
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) stop();
  });
  window.addEventListener("pagehide", stop);
  example.hidden = false;
}
