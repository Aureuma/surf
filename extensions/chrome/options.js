const input = document.getElementById("relayUrl");
chrome.storage.sync.get(["relayUrl"]).then(({ relayUrl }) => {
  input.value = relayUrl || "http://127.0.0.1:8932";
});
document.getElementById("save")?.addEventListener("click", async () => {
  await chrome.storage.sync.set({ relayUrl: input.value.trim() });
});
