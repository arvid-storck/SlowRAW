import clsx from 'clsx';
import { useId } from 'react';
import { motion } from 'framer-motion';
import Text from './Text';
import { TextVariants } from '../../types/typography';

interface SwitchProps {
  checked: boolean;
  className?: string;
  disabled?: boolean;
  id?: string;
  label: string;
  onChange(val: boolean): any;
  tooltip?: string;
  trackClassName?: string;
}

/**
 * A beautiful, reusable, and accessible toggle switch component.
 *
 * @param {string} label - The text label for the switch.
 * @param {boolean} checked - The current state of the switch.
 * @param {function(boolean): void} onChange - Callback function that receives the new boolean state.
 * @param {boolean} [disabled=false] - Whether the switch is interactive.
 * @param {string} [className=''] - Additional classes for the container.
 * @param {string} [trackClassName] - Custom classes for the switch's background track.
 */
const Switch = ({
  checked,
  className = '',
  disabled = false,
  id,
  label,
  onChange,
  tooltip,
  trackClassName,
}: SwitchProps) => {
  const generatedId = useId();
  const uniqueId = id ?? generatedId;

  const spring = {
    type: 'spring',
    stiffness: 700,
    damping: 30,
  } as const;

  return (
    <label
      className={clsx(
        'flex min-w-0 items-center justify-between gap-3',
        disabled ? 'cursor-not-allowed opacity-50' : 'cursor-pointer',
        className,
      )}
      htmlFor={uniqueId}
      data-tooltip={tooltip}
    >
      <Text variant={TextVariants.label} className="min-w-0 flex-1 select-none">
        {label}
      </Text>
      <div className="relative w-10 h-5 shrink-0">
        <input
          checked={checked}
          className="sr-only peer"
          disabled={disabled}
          id={uniqueId}
          onChange={(e: any) => !disabled && onChange(e.target.checked)}
          type="checkbox"
          role="switch"
        />
        <div
          className={clsx(
            'w-full h-full bg-card-active/50 rounded-full switch-round shadow-inner peer-focus-visible:ring-2 peer-focus-visible:ring-accent',
            trackClassName,
          )}
        ></div>
        <motion.div
          className={clsx('absolute top-0.5 left-0.5 w-4 h-4 rounded-full switch-round transition-colors', {
            'bg-accent': checked,
            'bg-text-secondary/80': !checked,
          })}
          transition={spring}
          initial={false}
          animate={{ x: checked ? 20 : 0 }}
        >
          <svg
            aria-hidden="true"
            viewBox="0 0 16 16"
            className="w-4 h-4 text-surface"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.7"
          >
            {checked ? <path d="M8 4v8" /> : <circle cx="8" cy="8" r="3.3" />}
          </svg>
        </motion.div>
      </div>
    </label>
  );
};

export default Switch;
