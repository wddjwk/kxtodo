<script lang="ts">
  /**
   * 一笔账的右键/长按菜单：编辑、改日期、改账户、删除。
   * 写入全走 actions（core 命令层），菜单自己只负责收集意图。
   */
  import { createEventDispatcher } from "svelte";
  import { CalendarDays, PenLine, Trash2, Wallet } from "@lucide/svelte";
  import ContextMenu from "../menu/ContextMenu.svelte";
  import MenuItem from "../menu/MenuItem.svelte";
  import MenuSeparator from "../menu/MenuSeparator.svelte";
  import DatePicker from "../DatePicker.svelte";
  import type { LedgerBook, LedgerEntry } from "../types";
  import { updateLedgerEntry, deleteLedgerEntry } from "../actions";

  export let x = 0;
  export let y = 0;
  export let entry: LedgerEntry;
  export let book: LedgerBook;

  const dispatch = createEventDispatcher<{ edit: string; close: void }>();

  function close(): void {
    dispatch("close");
  }

  function edit(): void {
    dispatch("edit", entry.id);
    close();
  }

  async function setDate(date: string): Promise<void> {
    await updateLedgerEntry(entry.id, { date });
    close();
  }

  async function setAccount(accountId: string): Promise<void> {
    await updateLedgerEntry(entry.id, { accountId });
    close();
  }

  async function remove(): Promise<void> {
    await deleteLedgerEntry(entry.id);
    close();
  }
</script>

<ContextMenu {x} {y} minWidth={216} onClose={close}>
  <MenuItem icon={PenLine} label="编辑这一笔" onSelect={edit} />
  <MenuItem icon={CalendarDays} label="修改日期">
    <div slot="submenu" class="task-menu-date">
      <DatePicker value={entry.date} on:select={(event) => setDate(event.detail)} />
    </div>
  </MenuItem>
  {#if entry.kind !== "transfer"}
    <MenuItem icon={Wallet} label="改账户">
      <div slot="submenu" class="ledger-menu-accounts">
        {#each book.accounts as account (account.id)}
          <MenuItem
            label={account.name}
            active={account.id === entry.accountId}
            onSelect={() => setAccount(account.id)}
          />
        {/each}
      </div>
    </MenuItem>
  {/if}
  <MenuSeparator />
  <MenuItem icon={Trash2} danger label="删除这一笔" onSelect={remove} />
</ContextMenu>
