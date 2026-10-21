import { useAuth } from '../components/AuthContext';
import { useSession } from '../api/queries';

export function useCurrentSession() {
  const { token } = useAuth();
  return useSession(token);
}
