declare namespace svelteHTML {
  interface IntrinsicElements {
    "emoji-picker": {
      class?: string;
      "data-source"?: string;
      locale?: string;
      "on:emoji-click"?: (event: CustomEvent<{ unicode: string }>) => void;
      onemojiclick?: (event: CustomEvent<{ unicode: string }>) => void;
    };
  }
}
