import { 
  FileText, GitBranch, Database, FileText as FileTextIcon, 
  Plus, Eye, Edit, Download 
} from 'lucide-react';
import { useNavigate } from 'react-router-dom';
import { Card } from '../components/ui/Card';
import { Button } from '../components/ui/Button';
import { useProjectStore } from '../stores/projectStore';

const recentProjects = [
  { name: 'Customer Portal', type: 'Web App', updated: '2 hours ago', status: 'Active' },
  { name: 'Admin Dashboard', type: 'Internal Tool', updated: '1 day ago', status: 'Draft' },
  { name: 'Mobile App', type: 'React Native', updated: '3 days ago', status: 'Active' },
  { name: 'API Gateway', type: 'Backend', updated: '1 week ago', status: 'Archived' },
];

export function Dashboard() {
  const navigate = useNavigate();
  const { project, newProject } = useProjectStore();

  const entityCount = project ? Object.keys(project.entities).length : 0;
  const stats = [
    { title: 'Forms', value: String(Object.keys((project?.forms as Record<string, unknown>) ?? {}).length), icon: FileText, color: 'text-blue-600', bg: 'bg-blue-100' },
    { title: 'Workflows', value: String(Object.keys((project?.workflows as Record<string, unknown>) ?? {}).length), icon: GitBranch, color: 'text-purple-600', bg: 'bg-purple-100' },
    { title: 'Data Models', value: String(entityCount), icon: Database, color: 'text-green-600', bg: 'bg-green-100' },
    { title: 'Pages', value: String(Object.keys((project?.pages as Record<string, unknown>) ?? {}).length), icon: FileTextIcon, color: 'text-orange-600', bg: 'bg-orange-100' },
  ];

  const handleNewProject = async () => {
    const name = window.prompt('Project name', 'My App');
    if (name) {
      await newProject(name);
      navigate('/data');
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-3xl font-bold text-gray-900 dark:text-white">
            {project?.name ?? 'Dashboard'}
          </h1>
          <p className="text-gray-600 dark:text-gray-400 mt-1">
            {project?.description ?? 'Overview of your no-code projects'}
          </p>
        </div>
        <Button variant="primary" onClick={() => void handleNewProject()}>
          <Plus className="w-4 h-4 mr-2" />
          New Project
        </Button>
      </div>

      {/* Stats Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        {stats.map((stat) => (
          <Card key={stat.title} className="p-6">
            <div className="flex items-center justify-between">
              <div>
                <p className="text-sm font-medium text-gray-600 dark:text-gray-400">{stat.title}</p>
                <p className="text-3xl font-bold text-gray-900 dark:text-white mt-1">{stat.value}</p>
              </div>
              <div className={`p-3 rounded-lg ${stat.bg} dark:bg-opacity-30`}>
                <stat.icon className={`w-6 h-6 ${stat.color}`} />
              </div>
            </div>
          </Card>
        ))}
      </div>

      {/* Quick Actions */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        {[
          { title: 'Create Form', description: 'Build data collection forms', icon: FileText, href: '/forms' },
          { title: 'Design Workflow', description: 'Visual process automation', icon: GitBranch, href: '/workflows' },
          { title: 'Model Data', description: 'Define entities and relations', icon: Database, href: '/data' },
          { title: 'Build Pages', description: 'Create responsive UI', icon: FileTextIcon, href: '/pages' },
        ].map((action) => (
          <Card key={action.title} className="p-6 hover:shadow-md transition-shadow cursor-pointer">
            <div className="flex items-start gap-4">
              <div className="p-3 bg-primary-100 dark:bg-primary-900/30 rounded-lg">
                <action.icon className="w-6 h-6 text-primary-600 dark:text-primary-400" />
              </div>
              <div className="flex-1">
                <h3 className="font-semibold text-gray-900 dark:text-white">{action.title}</h3>
                <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">{action.description}</p>
              </div>
            </div>
          </Card>
        ))}
      </div>

      {/* Recent Projects */}
      <Card>
        <div className="p-6 border-b border-gray-200 dark:border-slate-700">
          <div className="flex items-center justify-between">
            <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Recent Projects</h2>
            <Button variant="ghost" size="sm">View All</Button>
          </div>
        </div>
        <div className="overflow-x-auto">
          <table className="w-full">
            <thead>
              <tr className="border-b border-gray-200 dark:border-slate-700">
                <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Project</th>
                <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Type</th>
                <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Last Updated</th>
                <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Status</th>
                <th className="px-6 py-3 text-right text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
              {recentProjects.map((project) => (
                <tr key={project.name} className="hover:bg-gray-50 dark:hover:bg-slate-800/50">
                  <td className="px-6 py-4">
                    <div className="font-medium text-gray-900 dark:text-white">{project.name}</div>
                  </td>
                  <td className="px-6 py-4 text-gray-600 dark:text-gray-400">{project.type}</td>
                  <td className="px-6 py-4 text-gray-600 dark:text-gray-400">{project.updated}</td>
                  <td className="px-6 py-4">
                    <span className={`inline-flex px-2 py-1 text-xs font-semibold rounded-full ${
                      project.status === 'Active' 
                        ? 'bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400' 
                        : project.status === 'Draft'
                        ? 'bg-yellow-100 text-yellow-800 dark:bg-yellow-900/30 dark:text-yellow-400'
                        : 'bg-gray-100 text-gray-800 dark:bg-gray-900/30 dark:text-gray-400'
                    }`}>
                      {project.status}
                    </span>
                  </td>
                  <td className="px-6 py-4 text-right">
                    <div className="flex items-center justify-end gap-2">
                      <Button variant="ghost" size="sm"><Eye className="w-4 h-4" /></Button>
                      <Button variant="ghost" size="sm"><Edit className="w-4 h-4" /></Button>
                      <Button variant="ghost" size="sm"><Download className="w-4 h-4" /></Button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Card>
    </div>
  );
}