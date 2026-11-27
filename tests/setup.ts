const store = new Map<string, string>();

const storageMock: Storage = {
  get length() {
    return store.size;
  },
  clear() {
    store.clear();
  },
  getItem(k: string) {
    return store.has(k) ? store.get(k)! : null;
  },
  key(i: number) {
    return Array.from(store.keys())[i] ?? null;
  },
  removeItem(k: string) {
    store.delete(k);
  },
  setItem(k: string, v: string) {
    store.set(k, String(v));
  },
};

const documentMock = {
  documentElement: { dataset: {} as Record<string, string> },
  getElementById: () => null,
  querySelector: () => null,
  querySelectorAll: () => [],
  createElement: () => ({
    className: '',
    dataset: {},
    style: {},
    appendChild() {},
    addEventListener() {},
  }),
  addEventListener() {},
  removeEventListener() {},
};

const windowMock = {
  addEventListener() {},
  removeEventListener() {},
  matchMedia: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }),
};

Object.defineProperty(globalThis, 'localStorage', { value: storageMock, configurable: true });
Object.defineProperty(globalThis, 'document', { value: documentMock, configurable: true });
Object.defineProperty(globalThis, 'window', { value: windowMock, configurable: true });

export { store };
