/**
 * Generate a deterministic avatar based on an identity hash.
 */

export interface FlowShard {
  type: 'polygon' | 'arc' | 'grain';
  points?: string;
  x: number;
  y: number;
  r?: number;
  d?: string;
  color: string;
  opacity: number;
  rotation: number;
  blendMode: 'screen' | 'overlay' | 'multiply' | 'normal';
}

export interface SigilData {
  baseBg: string;
  shards: FlowShard[];
}

const v = (hash: string, s: number, l: number, m: number) => parseInt(hash.substring(s % 60, (s + l) % 60) || '0', 16) % m;

const CYBER_THEMES = [
  ['#00ff9f', '#001aff', '#d600ff'], // Neon Cyan/Blue/Pink
  ['#ff0055', '#ff9f00', '#000000'], // Tech Red/Gold/Black
  ['#7209b7', '#3f37c9', '#4cc9f0'], // Purple/DeepBlue/Sky
  ['#39ff14', '#053100', '#ffffff'], // Matrix Green/Dark/White
  ['#ff5d00', '#ff0000', '#000000'], // Warning Orange/Red
];

export const getAvatarData = (hash: string): SigilData => {
  const theme = CYBER_THEMES[v(hash, 0, 1, CYBER_THEMES.length)];
  const shards: FlowShard[] = [];
  
  const numShards = 12 + v(hash, 2, 1, 8);
  for (let i = 0; i < numShards; i++) {
    const seed = v(hash, i * 2, 3, 1000);
    const numPoints = 3 + (seed % 3);
    let points = "";
    for (let j = 0; j < numPoints; j++) {
      points += `${v(hash, i + j, 2, 120)-10},${v(hash, i + j + 5, 2, 120)-10} `;
    }
    
    shards.push({
      type: 'polygon',
      points,
      x: 0, y: 0,
      color: theme[seed % theme.length],
      opacity: 0.2 + (seed % 60 / 100),
      rotation: v(hash, i + 10, 2, 360),
      blendMode: (seed % 10 < 3) ? 'screen' : 'normal'
    });
  }

  const numFlows = 15 + v(hash, 5, 1, 15);
  const cx = 30 + v(hash, 8, 2, 40);
  const cy = 30 + v(hash, 10, 2, 40);

  for (let i = 0; i < numFlows; i++) {
    const seed = v(hash, i + 30, 3, 1000);
    const px = v(hash, i + 35, 2, 100);
    const py = v(hash, i + 40, 2, 100);
    
    const dx = px - cx;
    const dy = py - cy;
    const angle = Math.atan2(dy, dx) + (Math.PI / 2);
    const dist = Math.sqrt(dx*dx + dy*dy);
    const rotation = (angle * 180 / Math.PI) + (seed % 30) - 15;
    
    shards.push({
      type: 'arc',
      x: px, y: py,
      r: 5 + (seed % 20),
      d: `M0 0 Q${10 + (seed % 10)} ${seed % 10 - 5} ${20 + (seed % 15)} 0`,
      color: theme[(seed + 1) % theme.length],
      opacity: 0.3 + (seed % 50 / 100),
      rotation,
      blendMode: 'overlay'
    });
  }

  return { baseBg: '#050505', shards };
};
