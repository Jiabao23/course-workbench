import { describe, expect, it } from 'vitest';
import { singleLocalFile } from './import-utils';

describe('native local file selection', () => {
  it('accepts Unicode, spaces and uppercase extensions without changing the path', () => {
    expect(singleLocalFile(['D:\\课程 文件\\第一课.MP4'])).toBe('D:\\课程 文件\\第一课.MP4');
    expect(singleLocalFile(['"D:/课程/字幕.SRT"'])).toBe('D:/课程/字幕.SRT');
  });
  it('requires exactly one file instead of silently ignoring a multiple-file drop', () => {
    expect(() => singleLocalFile([])).toThrow('没有检测');
    expect(() => singleLocalFile(['D:/one.wav', 'D:/two.wav'])).toThrow('一次请拖入一个');
  });
  it('rejects web URLs, relative paths and unsupported file types', () => {
    for (const path of ['https://example.org/video.mp4', 'file:///D:/video.mp4', '../lesson.wav', 'D:/document.pdf']) {
      expect(() => singleLocalFile([path])).toThrow();
    }
  });
});
