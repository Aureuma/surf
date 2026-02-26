const attachedTabs = new Set();

chrome.runtime.onMessage.addListener(async (msg) => {
  if (msg?.type !== "surf.toggle") return;
  const tabId = msg.tabId;
  if (!tabId) return;
  try {
    if (attachedTabs.has(tabId)) {
      await chrome.debugger.detach({ tabId });
      attachedTabs.delete(tabId);
      return;
    }
    await chrome.debugger.attach({ tabId }, "1.3");
    attachedTabs.add(tabId);
  } catch {
    // no-op
  }
});
