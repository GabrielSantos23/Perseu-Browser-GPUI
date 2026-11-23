(() => {
  if (!self.chrome || !chrome.windows || !chrome.windows.create || !chrome.tabs) return;
  const create = chrome.windows.create.bind(chrome.windows);
  const own = chrome.runtime.getURL('/');
  const opened = async (before) => {
    for (let attempt = 0; attempt < 30; attempt++) {
      const tab = (await chrome.tabs.query({})).find(
        (tab) => !before.has(tab.id) && String(tab.url).startsWith(own),
      );
      if (tab) {
        return chrome.windows
          .get(tab.windowId, { populate: true })
          .catch(() => ({ id: tab.windowId, type: 'popup', tabs: [tab] }));
      }
      await new Promise((done) => setTimeout(done, 100));
    }
    return undefined;
  };
  chrome.windows.create = function (data, callback) {
    const result = chrome.tabs
      .query({})
      .then((tabs) => new Set(tabs.map((tab) => tab.id)))
      .then((before) => Promise.resolve(create(data)).then((window) => window || opened(before)));
    if (typeof callback !== 'function') return result;
    result.then(callback, () => callback(undefined));
  };
  for (const [api, key] of [[chrome.windows, 'windows'], [chrome.tabs, 'tabs']]) {
    if (!api.remove) continue;
    const remove = api.remove.bind(api);
    api.remove = function (ids, callback) {
      const close = () =>
        chrome.runtime.sendMessage({ __perseuClose: { [key]: [].concat(ids) } }).catch(() => {});
      const result = Promise.resolve(remove(ids)).then(
        (value) => (close(), value),
        (error) => {
          close();
          throw error;
        },
      );
      if (typeof callback !== 'function') return result;
      result.then(() => callback(), () => callback());
    };
  }
})();
