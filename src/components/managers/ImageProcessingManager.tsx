import { useImageProcessing } from '../../hooks/useImageProcessing';

interface Props {
  transformWrapperRef: React.RefObject<any>;
  prevAdjustmentsRef: React.RefObject<any>;
  previewJobIdRef: React.RefObject<number>;
  currentResRef: React.RefObject<number>;
}

export default function ImageProcessingManager(props: Props) {
  useImageProcessing(props.transformWrapperRef, props.prevAdjustmentsRef, {
    previewJobIdRef: props.previewJobIdRef,
    currentResRef: props.currentResRef,
  });

  return null;
}
