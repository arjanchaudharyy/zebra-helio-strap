import { Channel } from '@tauri-apps/api/core';
import { invoke } from './bridge/ipc';

const dragPreview = (label: string): string => {
  const canvas = document.createElement('canvas');
  const ratio = Math.min(2, window.devicePixelRatio || 1);
  canvas.width = 220 * ratio;
  canvas.height = 56 * ratio;
  const ctx = canvas.getContext('2d');
  if (!ctx) return canvas.toDataURL('image/png');
  ctx.scale(ratio, ratio);
  ctx.fillStyle = 'rgba(30, 35, 43, 0.92)';
  ctx.beginPath();
  ctx.roundRect(0, 0, 220, 56, 14);
  ctx.fill();
  ctx.fillStyle = '#A2C765';
  ctx.fillRect(14, 14, 20, 28);
  ctx.fillStyle = '#F2F4EE';
  ctx.font = '600 13px system-ui, sans-serif';
  const text = label.length > 22 ? `${label.slice(0, 21)}…` : label;
  ctx.fillText(text, 44, 33);
  return canvas.toDataURL('image/png');
};

export const startFileDrag = async (path: string, label: string): Promise<void> => {
  await invoke('plugin:drag|start_drag', {
    item: [path],
    image: dragPreview(label),
    onEvent: new Channel(),
  });
};
