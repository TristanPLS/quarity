import { useAuth } from './AuthContext'

/**
 * Droit d'écriture de l'utilisateur courant (RBAC `can_write` exposé par `/me`).
 *
 * `false` pour le rôle `lecteur` : l'UI doit désactiver les actions de mutation
 * (créer / éditer / supprimer) et signaler le mode lecture seule. Le back reste la
 * source de vérité — il renvoie 403 `read_only_role` si un lecteur force une mutation,
 * d'où le repli FR de {@link apiErrorMessage} sur ce statut.
 */
export function useCanWrite(): boolean {
  const { user } = useAuth()
  return user?.can_write ?? false
}
