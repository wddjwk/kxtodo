import { describe, expect, it } from "vitest";
import { externalLinkUrl, LINK_META_MAX_BYTES, normalizeLinkMeta, normalizeLinkUrl } from "../linkMeta";

describe("external link URL boundary", () => {
  it("normalizes cache keys but preserves navigation fragments", () => {
    expect(normalizeLinkUrl("https://EXAMPLE.com:443/a?q=1#part")).toBe("https://example.com/a?q=1");
    expect(externalLinkUrl("https://example.com/a#part")).toBe("https://example.com/a#part");
  });
  it.each([
    "javascript:alert(1)", "file:///secret", "tauri://localhost", "asset://localhost/x",
    "data:text/html,test", "mailto:a@example.com", "https://tauri.localhost/", "http://asset.localhost/x",
    "http://127.0.0.1:1420", "http://2130706433/", "http://[::1]/", "http://localhost./",
    "https://user:pass@example.com/", "https://example.com/a\nb", "not a URL"
  ])("rejects non-external URL %s", (url) => {
    expect(externalLinkUrl(url)).toBeNull();
    expect(normalizeLinkMeta(url, { title: "title" })).toBeNull();
  });
});

describe("metadata conversion", () => {
  const request = "https://www.example.com/article#section";
  it("keeps the requested identity and resolves redirect-relative icons", () => {
    expect(normalizeLinkMeta(request, JSON.stringify({
      url: "https://cdn.example.org/posts/final", title: "  Page\n title ",
      description: " An\t abstract ", site: "", icon: "../icon.png"
    }))).toEqual({
      url: "https://www.example.com/article", title: "Page title", description: "An abstract",
      site: "example.com", icon: "https://cdn.example.org/icon.png"
    });
  });
  it("cannot poison a different cache key with the returned URL", () => {
    expect(normalizeLinkMeta(request, { url: "https://victim.example/", title: "untrusted" })?.url)
      .toBe("https://www.example.com/article");
  });
  it("ignores an unsafe final URL and rejects unsafe icons", () => {
    expect(normalizeLinkMeta(request, { url: "file:///secret", title: "T", icon: "/icon.png" })?.icon)
      .toBe("https://www.example.com/icon.png");
    for (const icon of ["javascript:alert(1)", "data:image/svg+xml,test", "http://asset.localhost/a"]) {
      expect(normalizeLinkMeta(request, { title: "T", icon })?.icon).toBe("");
    }
  });
  it("defaults icons using the complete origin including its port", () => {
    expect(normalizeLinkMeta("https://example.com:8443/a", { title: "T" })?.icon)
      .toBe("https://example.com:8443/favicon.ico");
  });
  it("bounds text by code points without interpreting HTML", () => {
    const result = normalizeLinkMeta(request, { title: "𠮷".repeat(201), site: "s".repeat(101), description: "<b>literal</b>" });
    expect(result?.title).toBe("𠮷".repeat(200) + "…");
    expect(result?.site).toBe("s".repeat(100) + "…");
    expect(result?.description).toBe("<b>literal</b>");
  });
  it.each([null, [], "{bad", { title: 1 }, { title: {} }, {}, { title: "Just a moment..." }, { title: "Loading..." }])("rejects unusable input %j", (raw) => {
    expect(normalizeLinkMeta(request, raw)).toBeNull();
  });
  it("bounds both raw JSON bytes and decoded metadata", () => {
    expect(normalizeLinkMeta(request, " ".repeat(LINK_META_MAX_BYTES + 1))).toBeNull();
    expect(normalizeLinkMeta(request, { title: "字".repeat(LINK_META_MAX_BYTES / 2) })).toBeNull();
    expect(normalizeLinkMeta(request, { description: "description only" })?.title).toBe("");
  });
});
