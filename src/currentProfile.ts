import { t } from './i18n.ts';

export const CURRENT_PROFILE = '__current__';
export const profileLabel = (name: string) => name === CURRENT_PROFILE ? t('profiles.current') : name;
