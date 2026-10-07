function fig = plot_series_pub(vars, varargin)
%PLOT_SERIES_PUB  Plot publication-quality time series, simulations and transition paths from Dynare's oo_/M_.
%
%   Companion of plot_irfs_pub for path outputs other than IRFs: stochastic simulation
%   (stoch_simul with periods>0), perfect foresight transition paths, or any [endo x T]
%   path. Same layout as plot_irfs_pub: one panel per variable, one line per scenario,
%   compact tiledlayout, no box, color and line style both vary (distinct in black and
%   white), vector PDF export. A run script should call this function instead of inline
%   plot code.
%
%   Data source: oo_.<Source>, default 'endo_simul' (an [endo_nbr x T] matrix). The
%   stochastic simulation of stoch_simul and the perfect foresight path both go to
%   oo_.endo_simul, so one interface covers both. oo_.endo_simul also holds the initial
%   and terminal conditions: the first simulated period is column 1+M_.maximum_lag.
%
%   Simplest use: after the run, oo_/M_ are in the base workspace:
%       plot_series_pub({'y','c','k'});
%
%   Centered (subtract the steady state to show deviations):
%       plot_series_pub({'y','c'}, 'Center','ss', 'Scale',100, 'Save','fig_sim');
%   With Scale=100 this is a percent deviation only for variables in logs; for a
%   variable in levels it is 100 times the deviation in levels.
%
%   Compare scenarios (one line per scenario, distinct in black and white):
%       plot_series_pub({'y','c'}, 'Scenarios',{ooA,ooB}, ...
%                       'ScenarioNames',{'Baseline','Reform'}, 'Save','fig_transition');
%
%   Run without arguments to draw a demo figure from synthetic data and check the style:
%       plot_series_pub
%
% Main options (name-value pairs):
%   'oo_' / 'M_'      Dynare structures; default: read from the base workspace.
%   'Source'          field of oo_ that holds the [endo x T] path. Default 'endo_simul'.
%   'Scenarios'       cell {oo_1, oo_2, ...}: compare models or settings (one line each).
%                     When given, it replaces 'oo_'.
%   'ScenarioNames'   cell of legend names, one per scenario.
%   'Center'          'none' (default, raw levels) | 'ss' (subtract oo_.steady_state) |
%                     'mean' (subtract the sample mean). oo_.steady_state is filled by
%                     steady; after stoch_simul without steady; it still holds the initval
%                     values (the computed steady state is in oo_.dr.ys).
%   'Scale'           multiplier, default 1 (levels); with Center='ss', use 100 for
%                     percent deviations of log variables.
%   'Time'            custom x-axis vector; default 1:T. Give at least as many values
%                     as plotted periods.
%   'Horizon'         number of periods to plot; default: full length.
%   'Layout'          [nrows ncols]; default: near-square grid for the number of variables.
%   'Titles'          cell of panel titles; default: TeX name, then long_name, then name.
%   'Interpreter'     title interpreter: 'auto' (default) | 'latex' | 'tex' | 'none'.
%   'YLabel'/'XLabel' YLabel default follows Center ('Level' / 'Dev. from SS', or
%                     'Percent dev. from SS' when Scale is 100 / 'Dev. from mean');
%                     XLabel default 'Period'.
%   'Save'            file name without extension; 'Formats' default {'pdf'}, add 'eps'/'png'.
%   'Font'/'FontSize' default 'Helvetica' / 9.
%   'Grid'            default false. 'ZeroLine' default follows Center (drawn when centered).
%   'Colors'/'LineStyles'/'LineWidth'  override the default style.
%   'FigSize'         [width height] in centimeters; default: estimated from Layout.
%
% Returns: figure handle.
%
% Compatibility: tiledlayout/exportgraphics/yline need MATLAB R2020a or later. Older
% releases fall back to subplot + print + a drawn zero line (plainer style, still usable).
% Not tested under Octave.

% ----------------------------------------------------------------------
% 0. Demo without arguments
% ----------------------------------------------------------------------
if nargin == 0
    [vars, oo1, oo2, demo_M] = local_demo_data();
    varargin = {'oo_', oo1, 'M_', demo_M, ...
                'Scenarios', {oo1, oo2}, ...
                'ScenarioNames', {'Baseline','Reform'}};
end

% ----------------------------------------------------------------------
% 1. Parse arguments
% ----------------------------------------------------------------------
if ischar(vars) || isstring(vars), vars = cellstr(vars); end

p = inputParser;
p.addParameter('oo_', []);
p.addParameter('M_',  []);
p.addParameter('Source',        'endo_simul');
p.addParameter('Scenarios',     {});
p.addParameter('ScenarioNames', {});
p.addParameter('Center',        'none');   % 'none' | 'ss' | 'mean'
p.addParameter('Scale',         1);
p.addParameter('Time',          []);
p.addParameter('Horizon',       []);
p.addParameter('Layout',        []);
p.addParameter('Titles',        {});
p.addParameter('Interpreter',   'auto');
p.addParameter('YLabel',        '');       % empty = choose from Center
p.addParameter('XLabel',        'Period');
p.addParameter('Save',          '');
p.addParameter('Formats',       {'pdf'});
p.addParameter('Font',          'Helvetica');
p.addParameter('FontSize',      9);
p.addParameter('Grid',          false);
p.addParameter('ZeroLine',      []);       % empty = on when centered
p.addParameter('Colors',        []);
p.addParameter('LineStyles',    {'-','--',':','-.'});
p.addParameter('LineWidth',     1.6);
p.addParameter('FigSize',       []);
p.parse(varargin{:});
o = p.Results;

% Default oo_ / M_: read from the base workspace (a missing M_ becomes an empty struct)
if isempty(o.oo_) && isempty(o.Scenarios)
    o.oo_ = evalin('base','oo_');
end
if isempty(o.M_)
    try, o.M_ = evalin('base','M_'); catch, o.M_ = struct(); end
end
M_ = o.M_;
if ~isfield(M_,'endo_names'), M_.endo_names = {}; end

% Defaults that depend on Center (YLabel / ZeroLine)
ctr = lower(o.Center);
if isempty(o.YLabel)
    switch ctr
        case 'ss',   if o.Scale==100, o.YLabel = 'Percent dev. from SS'; else, o.YLabel = 'Dev. from SS'; end
        case 'mean', o.YLabel = 'Dev. from mean';
        otherwise,   o.YLabel = 'Level';
    end
end
if isempty(o.ZeroLine), o.ZeroLine = ~strcmp(ctr,'none'); end

% Scenario list: Scenarios when given (oo_ is not added), else the single oo_
if ~isempty(o.Scenarios), scen = o.Scenarios; else, scen = {o.oo_}; end
nScen = numel(scen);

% Legend names
if ~isempty(o.ScenarioNames)
    seriesNm = o.ScenarioNames;
elseif nScen > 1
    seriesNm = arrayfun(@(j)sprintf('Scenario %d',j), 1:nScen, 'uni',0);
else
    seriesNm = {''};
end

% Default colors (same as plot_irfs_pub)
if isempty(o.Colors)
    o.Colors = [0.12 0.24 0.45; 0.80 0.22 0.18; 0.20 0.55 0.35; ...
                0.50 0.38 0.66; 0.85 0.55 0.10; 0.35 0.35 0.35];
end

nv = numel(vars);
if isempty(o.Layout)
    nc = ceil(sqrt(nv)); nr = ceil(nv/nc);
else
    nr = o.Layout(1); nc = o.Layout(2);
end
useTiles = exist('tiledlayout','file')==2;

% ----------------------------------------------------------------------
% 2. Draw (one figure: one panel per variable, one line per scenario)
% ----------------------------------------------------------------------
fig = figure('Color','w','Units','centimeters');
if isempty(o.FigSize)
    w = max(8, nc*5.0 + 1.2);
    h = max(6, nr*3.8 + 1.4 + (nScen>1)*0.9);
    set(fig,'Position',[2 2 w h]);
else
    set(fig,'Position',[2 2 o.FigSize(1) o.FigSize(2)]);
end
if useTiles
    tl = tiledlayout(fig, nr, nc, 'TileSpacing','compact','Padding','compact');
end

hLeg = gobjects(1, nScen);
for i = 1:nv
    if useTiles, ax = nexttile(tl); else, ax = subplot(nr,nc,i,'Parent',fig); end
    hold(ax,'on');

    if o.ZeroLine
        if exist('yline','file')==2
            yl = yline(ax, 0, '-'); yl.Color = [0.6 0.6 0.6]; yl.LineWidth = 0.5;
            yl.Annotation.LegendInformation.IconDisplayStyle = 'off';
        else
            plot(ax, [0 1e6], [0 0], '-', 'Color',[0.6 0.6 0.6], 'LineWidth',0.5, ...
                 'HandleVisibility','off');
        end
    end

    for k = 1:nScen
        y = local_get_series(scen{k}, M_, vars{i}, o.Source, ctr);
        if isempty(y), continue; end
        H = o.Horizon; if isempty(H), H = numel(y); end
        H = min(H, numel(y));
        if isempty(o.Time), xx = (1:H)'; else, xx = o.Time(1:min(H,numel(o.Time)))'; end
        col = o.Colors(mod(k-1,size(o.Colors,1))+1, :);
        lst = o.LineStyles{mod(k-1,numel(o.LineStyles))+1};
        hl = plot(ax, xx, o.Scale*y(1:H), 'LineStyle',lst, 'Color',col, 'LineWidth',o.LineWidth);
        if i==1, hLeg(k) = hl; end
    end

    local_style_axis(ax, o);

    ttl = local_title(M_, vars{i}, o, i);
    [tstr, tint] = ttl{:};
    title(ax, tstr, 'Interpreter',tint, 'FontWeight','normal', ...
          'FontName',o.Font, 'FontSize',o.FontSize+1);

    isLeftCol   = mod(i-1, nc)==0;
    isBottomRow = (i > nv-nc);
    if isLeftCol,   ylabel(ax, o.YLabel, 'FontName',o.Font,'FontSize',o.FontSize); end
    if isBottomRow, xlabel(ax, o.XLabel, 'FontName',o.Font,'FontSize',o.FontSize); end

    hold(ax,'off');
end

if nScen > 1 && any(~cellfun(@isempty, seriesNm))
    if useTiles && exist('OCTAVE_VERSION','builtin')==0
        lgd = legend(hLeg, seriesNm, 'Orientation','horizontal', ...
                     'FontName',o.Font, 'FontSize',o.FontSize, 'Box','off');
        try, lgd.Layout.Tile = 'south'; catch, end
    else
        legend(hLeg, seriesNm, 'Box','off', 'FontName',o.Font, ...
               'FontSize',o.FontSize, 'Location','best');
    end
end

if ~isempty(o.Save)
    local_export(fig, o.Save, o.Formats);
end
end % ===== end of main function =====


% ======================================================================
% Local functions
% ======================================================================
function y = local_get_series(oo, M_, var, srcField, ctr)
% Return the full path of var from oo_.<srcField> ([endo x T]), centered as ctr says.
y = [];
if ~isfield(oo, srcField) || isempty(oo.(srcField)), return; end
names = cellstr(M_.endo_names);
idx = find(strcmp(names, var), 1);
if isempty(idx), return; end
M = oo.(srcField);
if idx > size(M,1), return; end
y = M(idx, :).';
switch ctr
    case 'ss'
        if isfield(oo,'steady_state') && idx <= numel(oo.steady_state)
            y = y - oo.steady_state(idx);
        end
    case 'mean'
        y = y - mean(y);
end
end

% ----------------------------------------------------------------------
function out = local_title(M_, var, o, i)
% Return {string, interpreter}. Order: Titles, TeX name, long_name, variable name.
% Dynare fills a TeX name for every variable (default: the name), so long_name is
% reached only when M_ has no TeX names.
if ~isempty(o.Titles) && numel(o.Titles) >= i && ~isempty(o.Titles{i})
    out = {o.Titles{i}, local_pick_interp(o.Interpreter,'none')}; return;
end
names = cellstr(M_.endo_names);
idx = find(strcmp(names, var), 1);
tex = ''; lng = '';
if ~isempty(idx)
    if isfield(M_,'endo_names_tex')
        t = M_.endo_names_tex; if iscell(t), tex = t{idx}; else, tex = strtrim(t(idx,:)); end
    end
    if isfield(M_,'endo_names_long')
        l = M_.endo_names_long; if iscell(l), lng = l{idx}; else, lng = strtrim(l(idx,:)); end
    end
end
if ~isempty(tex)
    out = {['$' tex '$'], local_pick_interp(o.Interpreter,'latex')};
elseif ~isempty(lng) && ~strcmp(lng, var)
    out = {lng, local_pick_interp(o.Interpreter,'none')};
else
    out = {var, local_pick_interp(o.Interpreter,'none')};
end
end

function intp = local_pick_interp(userInterp, autoChoice)
if strcmpi(userInterp,'auto'), intp = autoChoice; else, intp = userInterp; end
end

% ----------------------------------------------------------------------
function local_style_axis(ax, o)
set(ax, 'FontName',o.Font, 'FontSize',o.FontSize, 'Box','off', ...
        'TickDir','out', 'TickLength',[0.015 0.015], 'LineWidth',0.6, ...
        'Layer','top', 'XColor',[0.15 0.15 0.15], 'YColor',[0.15 0.15 0.15]);
if o.Grid
    grid(ax,'on'); set(ax,'GridAlpha',0.12, 'GridLineStyle',':');
else
    grid(ax,'off');
end
end

% ----------------------------------------------------------------------
function local_export(fig, base, formats)
for k = 1:numel(formats)
    fmt = lower(formats{k});
    fn  = [base '.' fmt];
    try
        if exist('exportgraphics','file')==2
            switch fmt
                case 'png', exportgraphics(fig, fn, 'Resolution',300);
                otherwise,  exportgraphics(fig, fn, 'ContentType','vector');
            end
        else
            set(fig,'PaperPositionMode','auto');
            switch fmt
                case 'pdf', print(fig, base, '-dpdf', '-painters');
                case 'eps', print(fig, base, '-depsc', '-painters');
                case 'png', print(fig, base, '-dpng', '-r300');
            end
        end
        fprintf('[plot_series_pub] Exported %s\n', fn);
    catch ME
        warning('plot_series_pub:export', 'Export of %s failed: %s', fn, ME.message);
    end
end
end

% ----------------------------------------------------------------------
function [vars, oo1, oo2, M] = local_demo_data()
% Synthetic transition paths (two scenarios) for the demo without arguments.
T = 40; t = (0:T-1)';
vars = {'y','c','k'};
path = @(ss,amp,p) ss + amp*(1 - exp(-t/p));
mk = @(s) [path(1.0,0.10*s,8)'; path(0.7,0.06*s,10)'; path(3.0,0.30*s,12)'];
oo1 = struct('endo_simul', mk(1.0), 'steady_state', [1.0;0.7;3.0]);
oo2 = struct('endo_simul', mk(1.6), 'steady_state', [1.0;0.7;3.0]);
M = struct();
M.endo_names      = {'y';'c';'k'};
M.endo_names_tex  = {'y';'c';'k'};
M.endo_names_long = {'Output';'Consumption';'Capital'};
end
