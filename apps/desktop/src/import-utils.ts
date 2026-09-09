export const LOCAL_EXTENSIONS = ['mp3','m4a','mp4','wav','flac','mkv','webm','ogg','opus','aac','mov','wma','srt','vtt','json'] as const;

export function singleLocalFile(paths: string[]): string {
  if (paths.length !== 1) throw new Error(paths.length ? '一次请拖入一个文件；多个文件可以分次导入。' : '没有检测到本地文件。');
  const path = paths[0].trim().replace(/^"(.*)"$/, '$1');
  if (/^[a-z][a-z\d+.-]*:\/\//i.test(path) || !/^(?:[a-z]:[\\/]|\\\\|\/)/i.test(path)) {
    throw new Error('请选择本地文件；网页链接请使用“其他视频网站”入口。');
  }
  const extension = path.split(/[\\/]/).pop()?.split('.').pop()?.toLowerCase();
  if (!LOCAL_EXTENSIONS.some(value => value === extension)) throw new Error('不支持此文件类型。请选择音视频、SRT、VTT 或 B 站字幕 JSON。');
  return path;
}
