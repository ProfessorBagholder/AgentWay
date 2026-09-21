// Apply the saved appearance before the stylesheet paints.
(() => {
  const key = "agentway-preview-theme";
  let theme = "dark";
  try {
    if (localStorage.getItem(key) === "light") theme = "light";
  } catch {}
  function apply() {
    document.documentElement.dataset.theme = theme;
    document.querySelectorAll('input[name="theme"]').forEach((input) => {
      input.checked = input.value === theme;
    });
  }
  apply();
  document.addEventListener("change", (event) => {
    if (!event.target.matches('input[name="theme"]')) return;
    theme = event.target.value === "light" ? "light" : "dark";
    apply();
    try {
      localStorage.setItem(key, theme);
    } catch {}
  });
  window.addEventListener("storage", (event) => {
    if (event.key !== key) return;
    theme = event.newValue === "light" ? "light" : "dark";
    apply();
  });
})();
