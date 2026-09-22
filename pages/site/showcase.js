const root = document.documentElement;
const params = new URLSearchParams(location.search);
const supportedViews = new Set(["timeline", "chat"]);
const currentView = supportedViews.has(params.get("view")) ? params.get("view") : "timeline";
const language = document.querySelector("#language");
const themeToggle = document.querySelector("#theme-toggle");
const status = document.querySelector("#demo-status");
const messageForm = document.querySelector("#mock-message-form");
const messageInput = document.querySelector("#mock-message");
const messageStream = document.querySelector("#message-stream");

const copy = {
  en: {
    conceptTitle: "Concept UI only",
    conceptBody: "Static mock data on GitHub Pages. This is not part of the social-service runtime and does not call a backend.",
    conceptLink: "What it demonstrates",
    timeline: "Timeline",
    messages: "Messages",
    groups: "Groups",
    saved: "Saved",
    poweredBy: "Illustrates APIs for",
    following: "Following",
    newPost: "New post",
    composerTitle: "Share something with your network",
    composerBody: "A real consumer could create public, owner-only, or approved-follower posts.",
    compose: "Compose",
    save: "Save",
    inbox: "Inbox",
    directConversation: "Direct conversation",
    send: "Send",
    architectureTitle: "What the concept is meant to communicate",
    architectureBody: "The Pages demo visualizes consumer experiences while keeping every authority boundary in the Rust/PostgreSQL service.",
    demoNotice: "This control is illustrative only; GitHub Pages does not mutate social-service data.",
    savedNotice: "Saved locally in this presentation only.",
    unsavedNotice: "Removed from the local presentation state.",
    messageNotice: "Mock message added locally. No request was sent to social-service."
  },
  de: {
    conceptTitle: "Nur Konzeptoberfläche",
    conceptBody: "Statische Beispieldaten auf GitHub Pages. Diese Oberfläche ist nicht Teil der social-service-Laufzeit und ruft kein Backend auf.",
    conceptLink: "Was gezeigt wird",
    timeline: "Timeline",
    messages: "Nachrichten",
    groups: "Gruppen",
    saved: "Gespeichert",
    poweredBy: "Zeigt APIs für",
    following: "Gefolgt",
    newPost: "Neuer Beitrag",
    composerTitle: "Etwas mit deinem Netzwerk teilen",
    composerBody: "Ein echter Client könnte öffentliche, private oder für bestätigte Follower sichtbare Beiträge erstellen.",
    compose: "Erstellen",
    save: "Speichern",
    inbox: "Postfach",
    directConversation: "Direkte Unterhaltung",
    send: "Senden",
    architectureTitle: "Was das Konzept vermitteln soll",
    architectureBody: "Die Pages-Demo visualisiert mögliche Clients, während alle Autoritätsgrenzen im Rust/PostgreSQL-Dienst bleiben.",
    demoNotice: "Dieses Element dient nur der Darstellung; GitHub Pages verändert keine social-service-Daten.",
    savedNotice: "Nur lokal in dieser Darstellung gespeichert.",
    unsavedNotice: "Aus dem lokalen Darstellungszustand entfernt.",
    messageNotice: "Beispielnachricht lokal hinzugefügt. Es wurde keine Anfrage an social-service gesendet."
  }
};

function setView(view, push = false) {
  const resolved = supportedViews.has(view) ? view : "timeline";
  document.querySelectorAll("[data-view]").forEach((node) => {
    node.hidden = node.dataset.view !== resolved;
  });
  document.querySelectorAll("[data-view-link]").forEach((node) => {
    if (node.dataset.viewLink === resolved) node.setAttribute("aria-current", "page");
    else node.removeAttribute("aria-current");
  });
  if (push) {
    const next = new URL(location.href);
    next.searchParams.set("view", resolved);
    history.pushState({ view: resolved }, "", next);
  }
}

function setLanguage(next) {
  const locale = copy[next] ? next : "en";
  root.lang = locale;
  language.value = locale;
  localStorage.setItem("social-service-pages.language", locale);
  document.querySelectorAll("[data-i18n]").forEach((node) => {
    const value = copy[locale][node.dataset.i18n];
    if (value) node.textContent = value;
  });
}

function setTheme(next) {
  const theme = next === "light" ? "light" : "dark";
  root.dataset.theme = theme;
  themeToggle.setAttribute("aria-pressed", theme === "dark" ? "true" : "false");
  localStorage.setItem("social-service-pages.theme", theme);
}

function announce(message) {
  status.textContent = message;
  status.hidden = false;
  clearTimeout(announce.timer);
  announce.timer = setTimeout(() => { status.hidden = true; }, 2600);
}

function localeCopy(key) {
  return copy[root.lang]?.[key] ?? copy.en[key];
}

document.querySelectorAll("[data-view-link]").forEach((link) => {
  link.addEventListener("click", (event) => {
    event.preventDefault();
    setView(link.dataset.viewLink, true);
  });
});

document.querySelectorAll("[data-demo-action=notice]").forEach((button) => {
  button.addEventListener("click", () => announce(localeCopy("demoNotice")));
});

document.querySelectorAll("[data-like]").forEach((button) => {
  button.addEventListener("click", () => {
    const active = button.getAttribute("aria-pressed") === "true";
    const count = button.querySelector("span");
    button.setAttribute("aria-pressed", String(!active));
    button.firstChild.textContent = active ? "♡ " : "♥ ";
    count.textContent = String(Number(count.textContent) + (active ? -1 : 1));
  });
});

document.querySelectorAll("[data-save]").forEach((button) => {
  button.addEventListener("click", () => {
    const active = button.getAttribute("aria-pressed") === "true";
    button.setAttribute("aria-pressed", String(!active));
    button.firstChild.textContent = active ? "☆ " : "★ ";
    announce(localeCopy(active ? "unsavedNotice" : "savedNotice"));
  });
});

messageForm.addEventListener("submit", (event) => {
  event.preventDefault();
  const value = messageInput.value.trim();
  if (!value) return;
  const message = document.createElement("div");
  message.className = "message message--self";
  const text = document.createElement("span");
  text.textContent = value;
  const time = document.createElement("time");
  time.textContent = new Intl.DateTimeFormat(root.lang, { hour: "2-digit", minute: "2-digit" }).format(new Date());
  message.append(text, time);
  messageStream.append(message);
  messageInput.value = "";
  message.scrollIntoView({ block: "nearest" });
  announce(localeCopy("messageNotice"));
});

language.addEventListener("change", () => setLanguage(language.value));
themeToggle.addEventListener("click", () => setTheme(root.dataset.theme === "dark" ? "light" : "dark"));
window.addEventListener("popstate", () => setView(new URLSearchParams(location.search).get("view")));

setTheme(localStorage.getItem("social-service-pages.theme") ?? "dark");
setLanguage(localStorage.getItem("social-service-pages.language") ?? "en");
setView(currentView);
