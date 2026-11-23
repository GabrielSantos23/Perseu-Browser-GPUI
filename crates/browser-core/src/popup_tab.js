(() => {
  if (!self.chrome || !chrome.tabs || !chrome.tabs.query) return;
  const query = chrome.tabs.query.bind(chrome.tabs);
  const ask = (info, callback) => (callback ? query(info, callback) : query(info));
  const page = async (info) => {
    const [own, all] = await Promise.all([chrome.tabs.getCurrent(), query({})]);
    const pages = all
      .filter((tab) => tab.id !== (own && own.id) && tab.url && !tab.url.startsWith('chrome-extension:'))
      .sort((a, b) => (b.lastAccessed || 0) - (a.lastAccessed || 0));
    return pages.length ? [{ ...pages[0], active: true }] : query(info);
  };
  chrome.tabs.query = function (info, callback) {
    if (!info || !info.active) return ask(info, callback);
    const result = page(info).catch(() => query(info));
    if (typeof callback !== 'function') return result;
    result.then(callback);
  };
})();
(() => {
  if (!self.chrome || !chrome.runtime || !chrome.runtime.onMessage || !chrome.tabs) return;
  // WebView2's windows.remove and tabs.remove leave a page the app showed
  // open; the worker script asks the page to close itself instead.
  chrome.runtime.onMessage.addListener((message, sender) => {
    const close = message && message.__perseuClose;
    if (!close || sender.id !== chrome.runtime.id) return;
    chrome.tabs.getCurrent().then((tab) => {
      if (tab && ((close.tabs || []).includes(tab.id) || (close.windows || []).includes(tab.windowId))) {
        window.close();
      }
    });
  });
})();
