document.getElementById("toggle")?.addEventListener("click", async () => {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab?.id) return;
  chrome.runtime.sendMessage({ type: "surf.toggle", tabId: tab.id });
  window.close();
});
