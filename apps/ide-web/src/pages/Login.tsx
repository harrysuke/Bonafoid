import { Link, useNavigate } from 'react-router-dom';
import { AuthCard, AuthForm } from './AuthForm';
import { login } from '../auth/session';

export function Login() {
  const navigate = useNavigate();

  return (
    <AuthCard
      title="No-Code IDE"
      subtitle="Sign in to your workspace"
      footer={
        <>
          No account?{' '}
          <Link to="/register" className="text-primary-600 hover:underline dark:text-primary-400">
            Create one
          </Link>
        </>
      }
    >
      <AuthForm
        mode="login"
        onSubmit={async (email, password) => {
          await login(email, password);
          navigate('/projects', { replace: true });
        }}
      />
    </AuthCard>
  );
}