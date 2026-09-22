try {
  document.documentElement.dataset.theme =
    localStorage.getItem("agentway-theme") === "light" ? "light" : "dark";
} catch {
  document.documentElement.dataset.theme = "dark";
}
