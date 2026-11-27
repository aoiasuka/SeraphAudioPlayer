/**
 * 字节 → base64 文本。给「把文件内容交给后端」用：数字数组形式的 JSON 参数体积是原文件的
 * 三倍多（每个字节变成「数字加逗号」），base64 只多三分之一，而且是一个字符串。
 *
 * 分块转换：`String.fromCharCode(...全部字节)` 在大数组上会超出调用栈的参数个数上限。
 */
export function bytesToBase64(bytes: Uint8Array): string {
  const CHUNK = 0x8000;
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + CHUNK));
  }
  return btoa(binary);
}
