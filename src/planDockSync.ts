export async function syncPlansFromFeishu(
  sync: () => Promise<string>,
  refresh: () => Promise<void>,
): Promise<string> {
  const message = await sync();
  await refresh();
  return message;
}
