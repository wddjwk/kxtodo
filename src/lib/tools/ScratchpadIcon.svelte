<script lang="ts">
  /**
   * 草稿纸的图标（需求 2「设计图标」）：一张带折角的纸 + 两道手写波浪线。
   * 笔画规格与 lucide 对齐（24 viewBox、stroke-width 2、圆头圆角），
   * 混在一排 lucide 图标里看不出是外来的；波浪线是「手写、不渲染」的意思。
   */
  export let size = 24;
  export let strokeWidth = 2;
</script>

<svg
  xmlns="http://www.w3.org/2000/svg"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={strokeWidth}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7z" />
  <path d="M15 2v5h5" />
  <path d="M8 12.5c1.2-1 2.3 1 3.5 0s2.3 1 3.5 0" />
  <path d="M8 16.5c1.2-1 2.3 1 3.5 0s2.3 1 3.5 0" />
</svg>
