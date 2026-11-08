'use strict';
// Browser history for the Observatory behavior fixture: a stack where pushState
// appends, replaceState rewrites the top entry, and back pops and dispatches
// popstate. The entry carries the view identity the app stored, so a test can
// assert both the restore and the cross-workspace guard.
module.exports = (events, listeners) => {
  const stack = [{ state: { wcodeView: 'overview', wcodeWorkspace: 'A' } }];
  let lastUrl = '';
  const listen = (name, handler) => {
    const callbacks = listeners.get(name) || [];
    callbacks.push(handler);
    listeners.set(name, callbacks);
    events[name] = event => { for (const callback of callbacks) callback(event); };
  };
  const history = {
    get length() { return stack.length; },
    get lastUrl() { return lastUrl; },
    pushState(state, title, url) { stack.push({ state }); lastUrl = url || ''; },
    replaceState(state, title, url) { stack[stack.length - 1] = { state }; lastUrl = url || ''; },
    back() {
      if (stack.length > 1) {
        stack.pop();
        lastUrl = '';
        events.popstate?.({ state: stack[stack.length - 1].state });
      }
    },
  };
  return { history, listen };
};
