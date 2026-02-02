import { computed, ref, watch } from "vue";

const TOKEN_KEY = "codeGatewayToken";

const readTokenFromHash = () => {
  const hash = window.location.hash.replace(/^#/, "");
  const params = new URLSearchParams(hash);
  const token = params.get("token");
  if (!token) {
    return null;
  }
  params.delete("token");
  const cleaned = params.toString();
  const url = cleaned
    ? `${window.location.pathname}#${cleaned}`
    : window.location.pathname;
  window.history.replaceState({}, "", url);
  return token;
};

export const useGatewayToken = () => {
  const fromHash = readTokenFromHash();
  const stored = localStorage.getItem(TOKEN_KEY) || "";
  const token = ref(fromHash || stored);

  if (fromHash) {
    localStorage.setItem(TOKEN_KEY, fromHash);
  }

  watch(token, (value) => {
    if (value) {
      localStorage.setItem(TOKEN_KEY, value);
    } else {
      localStorage.removeItem(TOKEN_KEY);
    }
  });

  const label = computed(() => {
    if (!token.value) return "No token";
    return `Token ${token.value.slice(0, 6)}…`;
  });

  const setToken = (value: string) => {
    token.value = value;
  };

  return { token, setToken, label };
};
