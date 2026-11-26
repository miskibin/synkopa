const compact = matchMedia("(max-width: 45rem)");
const menu = document.querySelector(".menu");
const syncMenu = () => { menu.open = !compact.matches; };
syncMenu();
compact.addEventListener("change", syncMenu);

const searchButton = document.querySelector("[data-search]");
const shortcut = /Mac|iPhone|iPad/.test(navigator.platform) ? "⌘ K" : "Ctrl K";
document.querySelector("[data-search-key]").textContent = shortcut;

function addDialog(className, label, markup) {
  const dialog = document.createElement("dialog");
  dialog.className = className;
  dialog.setAttribute("aria-label", label);
  dialog.innerHTML = markup;
  document.body.append(dialog);
  dialog.addEventListener("click", event => {
    if (event.target !== dialog) return;
    const rect = dialog.getBoundingClientRect();
    if (event.clientX < rect.left || event.clientX > rect.right ||
        event.clientY < rect.top || event.clientY > rect.bottom) dialog.close();
  });
  return dialog;
}

function setupSearch() {
  const panel = addDialog("search-panel", "Search guides", `
    <div class="query-row">
      <label for="query" class="visually-hidden">Search guides</label>
      <input id="query" type="search" placeholder="Search guides…" autocomplete="off">
      <form method="dialog"><button class="close">Close</button></form>
    </div>
    <div class="results"></div>
    <div class="visually-hidden" role="status"></div>
  `);
  const query = panel.querySelector("input");
  const results = panel.querySelector(".results");
  const status = panel.querySelector("[role=status]");
  const normalize = text => text.toLowerCase().normalize("NFKD").replace(/\p{Mark}/gu, "");
  const topics = Array.from(menu.querySelectorAll("nav a")).map(link => ({
    title: link.textContent,
    url: link.href,
    body: "",
  }));
  let entries = topics;
  let load;
  let loading = false;
  let limited = false;
  let selection = 0;
  let matches = [];
  let opener;

  async function indexGuides() {
    loading = true;
    const pages = await Promise.allSettled(topics.map(async topic => {
      const response = await fetch(topic.url);
      if (!response.ok) throw new Error("Could not load guide");
      const doc = new DOMParser().parseFromString(await response.text(), "text/html");
      return Array.from(doc.querySelectorAll(".prose section")).map(section => {
        const heading = section.querySelector("h2[id]");
        return {
          title: heading.textContent,
          url: new URL("#" + heading.id, topic.url).href,
          topic: topic.title,
          body: normalize(section.textContent),
        };
      });
    }));
    entries = [...topics, ...pages.flatMap(page => page.status === "fulfilled" ? page.value : [])];
    limited = pages.some(page => page.status === "rejected");
    loading = false;
    render();
  }

  function render() {
    const tokens = normalize(query.value).trim().split(/\s+/).filter(Boolean);
    matches = tokens.length ? entries.map(entry => {
      const title = normalize(entry.title);
      const topic = normalize(entry.topic || "");
      const ranks = tokens.map(token => title.includes(token) ? 4 :
        topic.includes(token) ? 2 : entry.body.includes(token) ? 1 : 0);
      return { entry, score: ranks.every(Boolean) ? ranks.reduce((sum, rank) => sum + rank, 0) : 0 };
    }).filter(result => result.score > 0).sort((a, b) => b.score - a.score).map(result => result.entry) : topics;
    matches = matches.slice(0, 12);
    selection = 0;
    results.replaceChildren();
    if (!matches.length) {
      const empty = document.createElement("p");
      empty.textContent = loading ? "Searching…" : "No matches.";
      results.append(empty);
    }
    for (const [index, entry] of matches.entries()) {
      const link = document.createElement("a");
      link.href = entry.url;
      link.textContent = entry.title;
      link.classList.toggle("active", index === selection);
      if (entry.topic) {
        const topic = document.createElement("small");
        topic.textContent = entry.topic;
        link.append(topic);
      }
      results.append(link);
    }
    if (limited) {
      const notice = document.createElement("p");
      notice.textContent = "Some guide sections are unavailable.";
      results.append(notice);
    }
    status.textContent = loading ? "Loading guides" : matches.length + " results";
  }

  function show() {
    if (panel.open) return;
    opener = document.activeElement;
    query.value = "";
    panel.showModal();
    query.focus();
    load ??= indexGuides();
    render();
  }

  panel.addEventListener("close", () => {
    if (opener instanceof HTMLElement) opener.focus({ preventScroll: true });
  });
  results.addEventListener("click", event => {
    if (event.target.closest("a") && !event.metaKey && !event.ctrlKey) panel.close();
  });
  query.addEventListener("input", render);
  query.addEventListener("keydown", event => {
    if (event.key === "Escape") {
      event.preventDefault();
      panel.close();
      return;
    }
    if (!matches.length) return;
    if (event.key === "Enter") {
      event.preventDefault();
      results.children[selection].click();
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      results.children[selection].classList.remove("active");
      selection = (selection + (event.key === "ArrowDown" ? 1 : -1) + matches.length) % matches.length;
      const selected = results.children[selection];
      selected.classList.add("active");
      selected.scrollIntoView({ block: "nearest" });
      status.textContent = selected.textContent;
    }
  });
  searchButton.hidden = false;
  searchButton.addEventListener("click", show);
  document.addEventListener("keydown", event => {
    if (event.key.toLowerCase() !== "k" || !(event.metaKey || event.ctrlKey)) return;
    if (document.querySelector("dialog[open]") && !panel.open) return;
    event.preventDefault();
    show();
  });
}

function setupScreenshots() {
  const shots = document.querySelectorAll(".shot");
  if (!shots.length) return;
  const panel = addDialog("lightbox", "Screenshot", `
    <form method="dialog">
      <a>Open full size</a>
      <button class="close">Close</button>
    </form>
    <img alt="">
  `);
  const preview = panel.querySelector("img");
  const fullSize = panel.querySelector("a");
  for (const shot of shots) {
    shot.classList.add("zoomable");
    shot.setAttribute("aria-haspopup", "dialog");
    shot.addEventListener("click", event => {
      if (event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
      event.preventDefault();
      preview.src = shot.href;
      preview.alt = shot.querySelector("img").alt;
      fullSize.href = shot.href;
      panel.showModal();
    });
  }
}

if (typeof HTMLDialogElement !== "undefined" && HTMLDialogElement.prototype.showModal) {
  setupSearch();
  setupScreenshots();
}

function setupPluginFilter() {
  const directory = document.querySelector(".directory");
  if (!directory) return;
  const label = directory.querySelector(".plugin-filter");
  const input = label.querySelector("input");
  const empty = directory.querySelector(".filter-empty");
  const status = directory.querySelector(".filter-status");
  const groups = Array.from(directory.querySelectorAll("section")).map(section => ({
    section,
    items: Array.from(section.querySelectorAll("li")).map(item => ({
      item,
      text: item.textContent.toLowerCase() + " " + section.querySelector("h2").textContent.toLowerCase(),
    })),
  }));
  label.hidden = false;
  input.addEventListener("input", () => {
    const words = input.value.trim().toLowerCase().split(/\s+/).filter(Boolean);
    let count = 0;
    for (const { section, items } of groups) {
      let matches = 0;
      for (const { item, text } of items) {
        item.hidden = !words.every(word => text.includes(word));
        if (!item.hidden) matches += 1;
      }
      section.hidden = matches === 0;
      count += matches;
    }
    empty.hidden = count !== 0;
    status.textContent = count === 0 ? "No matching plugins" : count + " results";
  });
}

setupPluginFilter();
