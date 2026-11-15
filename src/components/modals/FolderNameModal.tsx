import { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import Text from '../ui/Text';
import { TextVariants } from '../../types/typography';

interface FolderNameModalProps {
  currentName?: string;
  isOpen: boolean;
  onClose(): void;
  onSave(name: string): void;
  title?: string;
  placeholder?: string;
  buttonText?: string;
}

export default function FolderNameModal({
  currentName,
  isOpen,
  onClose,
  onSave,
  title,
  placeholder,
  buttonText,
}: FolderNameModalProps) {
  const { t } = useTranslation();
  const isRename = currentName !== undefined;
  const [name, setName] = useState('');
  const [isMounted, setIsMounted] = useState(false);
  const [show, setShow] = useState(false);

  useEffect(() => {
    if (isOpen) {
      if (currentName !== undefined) setName(currentName);
      setIsMounted(true);
      const timer = setTimeout(() => setShow(true), 10);
      return () => clearTimeout(timer);
    } else {
      setShow(false);
      const timer = setTimeout(() => {
        setIsMounted(false);
        setName('');
      }, 300);
      return () => clearTimeout(timer);
    }
  }, [isOpen, currentName]);

  const handleSave = useCallback(() => {
    const trimmed = name.trim();
    if (trimmed && trimmed !== currentName) {
      onSave(trimmed);
    }
    if (isRename || trimmed) onClose();
  }, [name, currentName, isRename, onSave, onClose]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        e.stopPropagation();
        handleSave();
      } else if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }
    },
    [handleSave, onClose],
  );

  if (!isMounted) {
    return null;
  }

  return (
    <div
      aria-modal="true"
      className={`
        fixed inset-0 flex items-center justify-center z-50
        bg-black/30 backdrop-blur-xs
        transition-opacity duration-300 ease-in-out
        ${show ? 'opacity-100' : 'opacity-0'}
      `}
      onClick={onClose}
      role="dialog"
    >
      <div
        className={`
          bg-surface rounded-lg shadow-xl p-6 w-full max-w-sm
          transform transition-all duration-300 ease-out
          ${show ? 'scale-100 opacity-100 translate-y-0' : 'scale-95 opacity-0 -translate-y-4'}
        `}
        onClick={(e: any) => e.stopPropagation()}
      >
        <Text variant={TextVariants.title} className="mb-4">
          {title || (isRename ? t('modals.renameFolder.title') : t('modals.createFolder.title'))}
        </Text>
        <input
          autoFocus
          className="w-full bg-bg-primary text-text-primary border border-border rounded-md px-3 py-2 focus:outline-hidden focus:ring-2 focus:ring-accent"
          onChange={(e: any) => setName(e.target.value)}
          onKeyDown={handleKeyDown}
          onFocus={isRename ? (e) => e.target.select() : undefined}
          placeholder={
            placeholder || (isRename ? t('modals.renameFolder.placeholder') : t('modals.createFolder.placeholder'))
          }
          type="text"
          value={name}
        />
        <div className="flex justify-end gap-3 mt-5">
          <button
            className="px-4 py-2 rounded-md text-text-secondary hover:bg-surface transition-colors"
            onClick={onClose}
          >
            {isRename ? t('modals.renameFolder.cancel') : t('modals.createFolder.cancel')}
          </button>
          <button
            className="px-4 py-2 rounded-md bg-accent text-button-text font-semibold hover:bg-accent-hover disabled:bg-gray-500 disabled:text-white disabled:cursor-not-allowed transition-colors"
            disabled={!name.trim() || name.trim() === currentName}
            onClick={handleSave}
          >
            {buttonText || (isRename ? t('modals.renameFolder.save') : t('modals.createFolder.create'))}
          </button>
        </div>
      </div>
    </div>
  );
}
