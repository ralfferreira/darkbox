import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import { useQueryClient } from '@tanstack/react-query';
import {
  clearToken as clearStoredToken,
  getToken,
  setToken as storeToken,
  UNAUTHORIZED_EVENT,
} from '../api/client';

interface AuthContextValue {
  token: string | null;
  rejection: string | null;
  signIn: (token: string) => void;
  signOut: () => void;
  clearRejection: () => void;
}

const AuthContext = createContext<AuthContextValue | null>(null);

export function useAuth(): AuthContextValue {
  const context = useContext(AuthContext);
  if (!context) throw new Error('useAuth must be used inside AuthProvider');
  return context;
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [token, setToken] = useState<string | null>(() => getToken());
  const [rejection, setRejection] = useState<string | null>(null);
  const queryClient = useQueryClient();

  const signIn = useCallback((next: string) => {
    storeToken(next);
    setRejection(null);
    setToken(next);
  }, []);

  const signOut = useCallback(() => {
    clearStoredToken();
    setRejection(null);
    setToken(null);
    queryClient.clear();
  }, [queryClient]);

  const clearRejection = useCallback(() => setRejection(null), []);

  useEffect(() => {
    const handler = () => {
      setToken(null);
      setRejection('The API rejected this token. Sign in with a valid bearer token.');
    };
    window.addEventListener(UNAUTHORIZED_EVENT, handler);
    return () => window.removeEventListener(UNAUTHORIZED_EVENT, handler);
  }, []);

  const value = useMemo(
    () => ({ token, rejection, signIn, signOut, clearRejection }),
    [token, rejection, signIn, signOut, clearRejection],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}
