// 言葉の切り替えと、Pagefind の検索を、mdBook の上の帯に足す。
(() => {
  const LANGS = { ja: "日本語", en: "English" };
  const SOURCE = "ja"; // site の root に出る言葉。ほかは /<lang>/
  const lang = document.documentElement.lang;
  const siteRoot = new URL((path_to_root || "./") + (lang === SOURCE ? "" : "../"), location.href);
  const bookRoot = new URL(path_to_root || "./", location.href);
  const page = location.href.slice(bookRoot.href.length);
  const left = document.querySelector("#mdbook-menu-bar .left-buttons");
  const right = document.querySelector("#mdbook-menu-bar .right-buttons");
  if (!left || !right) return;

  for (const [code, label] of Object.entries(LANGS)) {
    if (code === lang) continue;
    const a = document.createElement("a");
    a.href = new URL((code === SOURCE ? "" : code + "/") + page, siteRoot).href;
    a.textContent = label;
    a.lang = code;
    a.className = "lang-link";
    right.prepend(a);
  }

  const button = document.createElement("button");
  button.type = "button";
  button.className = "icon-button search-open";
  button.title = lang === "ja" ? "検索（/）" : "Search (/)";
  button.setAttribute("aria-label", button.title);
  button.textContent = "⌕";
  left.append(button);
  const dialog = document.createElement("dialog");
  dialog.className = "search-dialog";
  dialog.innerHTML = '<div id="pagefind-search"></div>';
  document.body.append(dialog);
  dialog.addEventListener("click", (e) => e.target === dialog && dialog.close());
  let loaded;
  const open = async () => {
    const bundle = new URL("pagefind/", siteRoot).href;
    loaded ??= new Promise((resolve, reject) => {
      const css = document.createElement("link");
      css.rel = "stylesheet";
      css.href = bundle + "pagefind-ui.css";
      const js = document.createElement("script");
      js.src = bundle + "pagefind-ui.js";
      js.onload = () => {
        new PagefindUI({ element: "#pagefind-search", bundlePath: bundle, showImages: false, resetStyles: false });
        resolve();
      };
      js.onerror = reject;
      document.head.append(css, js);
    });
    dialog.showModal();
    await loaded;
    dialog.querySelector("input")?.focus();
  };
  button.addEventListener("click", open);
  document.addEventListener("keydown", (e) => {
    if (e.key !== "/" || e.ctrlKey || e.metaKey || e.altKey) return;
    if (/^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement?.tagName ?? "")) return;
    e.preventDefault();
    open();
  });
})();
