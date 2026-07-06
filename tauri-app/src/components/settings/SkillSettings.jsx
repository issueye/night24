import { CheckCircle2, Copy, FileText, RefreshCw, Search, Sparkles, XCircle } from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { normalizeError } from '../../utils/events.js';
import { classNames } from '../../utils/format.js';
import { Button, IconButton, Select, Tag, TextField } from '../ui/index.js';
import { SettingsListDetail } from './SettingsListDetail.jsx';

const SOURCE_LABELS = {
  workspace: '工作区',
  project_agent: '项目',
  user: '用户',
};

function sourceLabel(source) {
  return SOURCE_LABELS[source] || source || '未知';
}

function availability(skill) {
  if (!skill?.enabled) return { label: '停用', tone: 'muted' };
  if (!skill?.eligible) return { label: '不可用', tone: 'error' };
  return { label: '可用', tone: 'success' };
}

function invocationText(skill) {
  if (!skill?.name) return '';
  return `$${skill.name}`;
}

export function SkillSettings({ apiJson, workspace }) {
  const [skills, setSkills] = useState([]);
  const [skillListVersion, setSkillListVersion] = useState(0);
  const [warnings, setWarnings] = useState([]);
  const [selectedName, setSelectedName] = useState('');
  const [loadedSkill, setLoadedSkill] = useState(null);
  const [query, setQuery] = useState('');
  const [sourceFilter, setSourceFilter] = useState('all');
  const [loading, setLoading] = useState(false);
  const [detailLoading, setDetailLoading] = useState(false);
  const [error, setError] = useState('');
  const [copied, setCopied] = useState('');
  const skillListRequestRef = useRef(0);
  const skillDetailRequestRef = useRef(0);

  const filteredSkills = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return skills.filter((skill) => {
      if (sourceFilter !== 'all' && skill.source !== sourceFilter) return false;
      if (!needle) return true;
      return [skill.name, skill.description, skill.path]
        .filter(Boolean)
        .some((value) => String(value).toLowerCase().includes(needle));
    });
  }, [query, skills, sourceFilter]);

  const selectedSkill = useMemo(
    () => skills.find((skill) => skill.name === selectedName) || filteredSkills[0] || null,
    [filteredSkills, selectedName, skills],
  );

  const counts = useMemo(() => {
    const enabled = skills.filter((skill) => skill.enabled && skill.eligible).length;
    const unavailable = skills.length - enabled;
    return { total: skills.length, enabled, unavailable };
  }, [skills]);

  async function loadSkills() {
    const requestId = skillListRequestRef.current + 1;
    skillListRequestRef.current = requestId;
    skillDetailRequestRef.current += 1;
    if (!workspace) {
      setSkills([]);
      setWarnings([]);
      setSelectedName('');
      setLoadedSkill(null);
      setDetailLoading(false);
      setError('');
      return;
    }
    setLoading(true);
    setLoadedSkill(null);
    setDetailLoading(false);
    setError('');
    setCopied('');
    try {
      const data = await apiJson('/workspace/skills');
      if (skillListRequestRef.current !== requestId) return;
      const registry = data?.registry || {};
      const nextSkills = Array.isArray(registry.skills) ? registry.skills : [];
      setSkills(nextSkills);
      setWarnings(Array.isArray(registry.warnings) ? registry.warnings : []);
      setSelectedName((current) => nextSkills.find((skill) => skill.name === current)?.name || nextSkills[0]?.name || '');
      setSkillListVersion((version) => version + 1);
    } catch (err) {
      if (skillListRequestRef.current !== requestId) return;
      setError(normalizeError(err));
      setSkills([]);
      setWarnings([]);
      setSelectedName('');
      setLoadedSkill(null);
    } finally {
      if (skillListRequestRef.current === requestId) setLoading(false);
    }
  }

  async function loadSkillDetail(skill) {
    const requestId = skillDetailRequestRef.current + 1;
    skillDetailRequestRef.current = requestId;
    if (!workspace || !skill?.name || !skill.enabled || !skill.eligible) {
      setLoadedSkill(null);
      setDetailLoading(false);
      return;
    }
    setLoadedSkill(null);
    setDetailLoading(true);
    setError('');
    try {
      const data = await apiJson(`/workspace/skills/${encodeURIComponent(skill.name)}`);
      if (skillDetailRequestRef.current !== requestId) return;
      setLoadedSkill(data?.skill || null);
    } catch (err) {
      if (skillDetailRequestRef.current !== requestId) return;
      setLoadedSkill(null);
      setError(normalizeError(err));
    } finally {
      if (skillDetailRequestRef.current === requestId) setDetailLoading(false);
    }
  }

  useEffect(() => {
    loadSkills();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspace?.root_path]);

  useEffect(() => {
    loadSkillDetail(selectedSkill);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedSkill?.name, selectedSkill?.eligible, selectedSkill?.enabled, skillListVersion, workspace?.root_path]);

  async function copyInvocation() {
    const text = invocationText(selectedSkill);
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      setCopied(text);
    } catch {
      setCopied('');
    }
  }

  if (!workspace) {
    return (
      <div className="hook-empty">
        <Sparkles size={18} />
        <strong>先打开项目</strong>
        <span>技能会从当前项目和用户目录加载。</span>
      </div>
    );
  }

  const status = availability(selectedSkill);
  const body = loadedSkill?.body || '';

  return (
    <SettingsListDetail
      managerClassName="skill-manager"
      listClassName="skill-list"
      listLabel="技能列表"
      listTitle="技能"
      listActions={(
        <IconButton className="icon-button compact" disabled={loading} label="重新加载" onClick={loadSkills} size="sm">
          <RefreshCw size={14} />
        </IconButton>
      )}
      listExtra={(
        <div className="skill-filter">
          <TextField className="skill-search-field" icon={<Search size={13} />} onChange={(event) => setQuery(event.target.value)} placeholder="搜索技能" value={query} />
          <Select
            onChange={setSourceFilter}
            options={[
              { label: '全部来源', value: 'all' },
              { label: '工作区', value: 'workspace' },
              { label: '项目', value: 'project_agent' },
              { label: '用户', value: 'user' },
            ]}
            value={sourceFilter}
          />
        </div>
      )}
      listChildren={(
        <>
          {filteredSkills.length === 0 && <div className="hook-list-empty">{loading ? '加载中' : '暂无技能'}</div>}
          {filteredSkills.map((skill) => {
            const itemStatus = availability(skill);
            return (
              <Button
                className={classNames('provider-profile-row skill-row', skill.name === selectedSkill?.name && 'active', itemStatus.tone)}
                key={`${skill.source}-${skill.name}`}
                onClick={() => setSelectedName(skill.name)}
                variant="ghost"
              >
                <strong>{skill.name}</strong>
                <span>{sourceLabel(skill.source)} · {itemStatus.label}</span>
              </Button>
            );
          })}
        </>
      )}
      detailClassName="skill-detail"
    >
        <div className="skill-summary">
          <div>
            <span>总数</span>
            <strong>{counts.total}</strong>
          </div>
          <div>
            <span>可用</span>
            <strong>{counts.enabled}</strong>
          </div>
          <div>
            <span>异常</span>
            <strong>{counts.unavailable}</strong>
          </div>
        </div>

        {error && <div className="hook-status error">{error}</div>}
        {warnings.length > 0 && (
          <div className="hook-status warning">
            {warnings.slice(0, 3).join('\n')}
          </div>
        )}

        {selectedSkill ? (
          <section className="skill-inspector">
            <header className="skill-inspector-head">
              <div>
                <strong>{selectedSkill.name}</strong>
                <span>{selectedSkill.description || '无描述'}</span>
              </div>
              <div className="skill-head-actions">
                <Tag className="skill-status" icon={status.tone === 'success' ? <CheckCircle2 size={13} /> : <XCircle size={13} />} tone={status.tone === 'error' ? 'danger' : status.tone}>
                  {status.label}
                </Tag>
                <Button className="toolbar-button compact-action" icon={<Copy size={14} />} onClick={copyInvocation} size="sm">
                  {copied ? '已复制' : '复制调用'}
                </Button>
              </div>
            </header>

            <div className="skill-meta-grid">
              <TextField label="来源" readOnly value={sourceLabel(selectedSkill.source)} />
              <TextField label="调用名" readOnly value={invocationText(selectedSkill)} />
              <TextField label="用户调用" readOnly value={selectedSkill.user_invocable ? '允许' : '关闭'} />
              <TextField label="模型调用" readOnly value={selectedSkill.model_invocable ? '允许' : '关闭'} />
              <TextField className="wide" label="目录" readOnly value={selectedSkill.base_dir || ''} />
              <TextField className="wide" label="SKILL.md" readOnly value={selectedSkill.path || ''} />
            </div>

            {selectedSkill.missing?.length > 0 && (
              <div className="skill-missing">
                {selectedSkill.missing.map((item) => (
                  <Tag key={item} size="sm" tone="danger">{item}</Tag>
                ))}
              </div>
            )}

            <div className="skill-body-head">
              <span><FileText size={13} /> SKILL.md</span>
              <small>{detailLoading ? '加载中' : body ? `${body.length} 字符` : '无内容'}</small>
            </div>
            <pre className="skill-body">{body || (selectedSkill.eligible ? '暂无内容' : '技能不可用，无法加载正文')}</pre>
          </section>
        ) : (
          <div className="hook-empty inline">
            <Sparkles size={18} />
            <strong>没有技能</strong>
            <span>当前项目未发现可管理的技能。</span>
          </div>
        )}
    </SettingsListDetail>
  );
}
