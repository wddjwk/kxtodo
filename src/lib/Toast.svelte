<script lang="ts">
  import { toastMessage } from "./stores";
</script>

{#if $toastMessage}
  <div class="toast">{$toastMessage}</div>
{/if}
