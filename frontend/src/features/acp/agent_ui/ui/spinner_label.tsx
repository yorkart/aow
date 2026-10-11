// Frames and durations from the upstream implementation's ui::SpinnerLabel; the DOM owns the animation timer.
import { useEffect, useState } from 'react';

const variants = {
  dots: { frames: ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'], duration: 1000 },
  sand: { frames: ['⠁', '⠂', '⠄', '⡀', '⡈', '⡐', '⡠', '⣀', '⣁', '⣂', '⣄', '⣌', '⣔', '⣤', '⣥', '⣦', '⣮', '⣶', '⣷', '⣿', '⡿', '⠿', '⢟', '⠟', '⡛', '⠛', '⠫', '⢋', '⠋', '⠍', '⡉', '⠉', '⠑', '⠡', '⢁'], duration: 2000 },
};

export function SpinnerLabel({ variant = 'dots' }: { variant?: keyof typeof variants }) {
  const { frames, duration } = variants[variant];
  const [frame, setFrame] = useState(0);
  useEffect(() => {
    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
    let timer: number | undefined;
    const update = () => {
      window.clearInterval(timer); setFrame(0);
      if (!reducedMotion.matches) timer = window.setInterval(() => setFrame(index => (index + 1) % frames.length), duration / frames.length);
    };
    update(); reducedMotion.addEventListener('change', update);
    return () => { window.clearInterval(timer); reducedMotion.removeEventListener('change', update); };
  }, [frames, duration]);
  return <span className="acp-spinner-label" aria-hidden="true">{frames[frame % frames.length]}</span>;
}
