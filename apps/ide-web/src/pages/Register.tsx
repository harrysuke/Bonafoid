import { Link, useNavigate } from 'react-router-dom';
import { AuthCard, AuthForm } from './AuthForm';
import { register } from '../auth/session';

export function Register() {
  const navigate = useNavigate();

  return (
    <AuthCard
      title="Create your account"
      subtitle="Projects are stored server-side and scoped to you"
      footer={
        <>
          Already registered?{' '}
          <Link to="/login" className="text-primary-600 hover:underline dark:text-primary-400">
            Sign in
          </Link>
        </>
      }
    >
      <AuthForm
        mode="register"
        onSubmit={async (email, password) => {
          await register(email, password);
          navigate('/projects', { replace: true });
        }}
      />
    </AuthCard>
  );
}