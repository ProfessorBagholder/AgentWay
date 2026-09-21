// Apply the saved appearance before the stylesheet paints.
(() => {
  const key = "agentway-preview-theme";
  let theme = "dark";
  try {
    if (localStorage.getItem(key) === "light") theme = "light";
  } catch {}
  document.documentElement.dataset.theme = theme;
  document.addEventListener("DOMContentLoaded", () => {
    const toggle = document.querySelector("#theme-toggle");
    toggle.checked = theme === "dark";
    toggle.addEventListener("change", () => {
      theme = toggle.checked ? "dark" : "light";
      document.documentElement.dataset.theme = theme;
      try {
        localStorage.setItem(key, theme);
      } catch {}
    });
    window.addEventListener("storage", (event) => {
      if (event.key !== key) return;
      theme = event.newValue === "light" ? "light" : "dark";
      document.documentElement.dataset.theme = theme;
      toggle.checked = theme === "dark";
    });
  });
})();
