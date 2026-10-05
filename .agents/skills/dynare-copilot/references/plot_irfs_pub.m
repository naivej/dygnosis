function fig = plot_irfs_pub(vars, shocks, varargin)
%PLOT_IRFS_PUB  Plot publication-quality impulse responses (IRFs) from Dynare's oo_/M_.
%
%   Reads oo_.irfs.<var>_<shock>, written by stoch_simul, and lays out one panel per
%   variable with one line per scenario or shock. The default style follows figures in
%   AER/JME/Econometrica: compact tiledlayout, zero line, no box, faint or no grid,
%   color and line style both vary (lines stay distinct in black-and-white print),
%   vector PDF export.
%
%   Simplest use: after stoch_simul, oo_/M_ are in the base workspace:
%       plot_irfs_pub({'y','c','invest','l'}, 'eps_z');
%
%   Compare scenarios (same variables, same shock, one line per scenario):
%       plot_irfs_pub({'y','pi','r'}, 'eps_a', ...
%           'Scenarios',     {oo_base, oo_alt}, ...
%           'ScenarioNames', {'Baseline','Sticky wages'}, ...
%           'Save', 'fig_irf_techshock');
%
%   Overlay several shocks in one figure (one color per shock in each panel):
%       plot_irfs_pub({'y','c'}, {'eps_a','eps_g'}, 'OverlayShocks', true);
%
%   Run without arguments to draw a demo figure from synthetic data and check the style:
%       plot_irfs_pub
%
% Main options (name-value pairs):
%   'oo_' / 'M_'      Dynare structures; default: read from the base workspace.
%   'Scenarios'       cell {oo_1, oo_2, ...}: compare models or settings (one line each).
%                     When given, it replaces 'oo_'.
%   'ScenarioNames'   cell of legend names, one per scenario.
%   'OverlayShocks'   true: overlay all shocks in one figure (not with Scenarios). Default false.
%   'Bands'           shaded bands, drawn only with one scenario and OverlayShocks false.
%                     {lower, upper}, each a struct with fields <var>_<shock> (for example
%                     oo_.PosteriorIRF.dsge.HPDinf and .HPDsup) or a numeric vector that is
%                     drawn in every panel (an H x nv matrix is not split by column).
%                     Two bands: nested cell {{l90,u90},{l68,u68}} (wide band first, light
%                     grey; narrow band second, darker grey).
%   'Scale'           IRF multiplier, default 100 (deviation of a log variable -> percent).
%                     Use 1 for variables in levels.
%   'Horizon'         number of periods to plot; default: full length.
%   'Layout'          [nrows ncols]; default: near-square grid for the number of variables.
%   'Titles'          cell of panel titles; default: TeX name, then long_name, then name.
%   'Interpreter'     title interpreter: 'auto' (default) | 'latex' | 'tex' | 'none'.
%   'YLabel'/'XLabel' default 'Percent dev. from SS' / 'Quarters'.
%   'Save'            file name without extension; 'Formats' default {'pdf'}, add 'eps'/'png'.
%   'Font'/'FontSize' default 'Helvetica' / 9.
%   'Grid'/'ZeroLine' default false / true.
%   'Colors'/'LineStyles'/'LineWidth'  override the default style.
%   'FigSize'         [width height] in centimeters; default: estimated from Layout.
%
% Returns: figure handle (one figure with OverlayShocks or one shock; with several shocks
% in separate figures, the last figure).
%
% Compatibility: tiledlayout/exportgraphics/yline need MATLAB R2020a or later. Older
% releases fall back to subplot + print + a drawn zero line (plainer style, still usable).
% Not tested under Octave.

% ----------------------------------------------------------------------
% 0. Demo without arguments
% ----------------------------------------------------------------------
if nargin == 0
    [vars, shocks, oo1, oo2, demo_M] = local_demo_data();
    varargin = {'oo_', oo1, 'M_', demo_M, ...
                'Scenarios', {oo1, oo2}, ...
                'ScenarioNames', {'Baseline','Low persistence'}};
end

% ----------------------------------------------------------------------
% 1. Parse arguments
% ----------------------------------------------------------------------
if nargin < 2 || isempty(shocks), shocks = {}; end
if ischar(vars) || isstring(vars),   vars   = cellstr(vars);   end
if ischar(shocks) || isstring(shocks), shocks = cellstr(shocks); end

p = inputParser;
p.addParameter('oo_', []);
p.addParameter('M_',  []);
p.addParameter('Scenarios',     {});
p.addParameter('ScenarioNames', {});
p.addParameter('OverlayShocks', false);
p.addParameter('Bands',         {});
p.addParameter('Scale',         100);
p.addParameter('Horizon',       []);
p.addParameter('Layout',        []);
p.addParameter('Titles',        {});
p.addParameter('Interpreter',   'auto');   % 'auto' | 'latex' | 'tex' | 'none'
p.addParameter('YLabel',        'Percent dev. from SS');
p.addParameter('XLabel',        'Quarters');
p.addParameter('Save',          '');
p.addParameter('Formats',       {'pdf'});
p.addParameter('Font',          'Helvetica');
p.addParameter('FontSize',      9);
p.addParameter('Grid',          false);
p.addParameter('ZeroLine',      true);
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
% Fill missing M_ fields; titles then fall back to variable and shock names
if ~isfield(M_,'endo_names'), M_.endo_names = {}; end
if ~isfield(M_,'exo_names'),  M_.exo_names  = {}; end

% Scenario list: Scenarios when given (oo_ is not added), else the single oo_
if ~isempty(o.Scenarios)
    scen = o.Scenarios;
else
    scen = {o.oo_};
end
nScen = numel(scen);

% Infer variables and shocks when not given
if isempty(vars) || isempty(shocks)
    [iv, ish] = local_infer(scen{1}, M_);
    if isempty(vars),   vars   = iv;  end
    if isempty(shocks), shocks = ish; end
end
nv = numel(vars);

% Default colors: dark to light, cool and warm alternate; colorblind-friendly and,
% with the line styles, distinct in black and white
if isempty(o.Colors)
    o.Colors = [0.12 0.24 0.45;    % navy
                0.80 0.22 0.18;    % brick red
                0.20 0.55 0.35;    % dark green
                0.50 0.38 0.66;    % purple
                0.85 0.55 0.10;    % amber
                0.35 0.35 0.35];   % grey
end

% ----------------------------------------------------------------------
% 2. Choose what varies across figures and across lines
% ----------------------------------------------------------------------
% Lines (series): the shocks with OverlayShocks; otherwise the scenarios.
% Figures: one figure with OverlayShocks; otherwise one figure per shock.
if o.OverlayShocks
    figLoop  = {[]};            % one figure
    seriesNm = shocks;
    seriesGet = @(k, sh, vr) local_get(scen{1}, vr, shocks{k});
    nSeries  = numel(shocks);
else
    figLoop  = shocks;
    if ~isempty(o.ScenarioNames)
        seriesNm = o.ScenarioNames;
    elseif nScen > 1
        seriesNm = arrayfun(@(j)sprintf('Scenario %d',j), 1:nScen, 'uni',0);
    else
        seriesNm = {''};
    end
    seriesGet = @(k, sh, vr) local_get(scen{k}, vr, sh);
    nSeries  = nScen;
end

% Panel grid
if isempty(o.Layout)
    nc = ceil(sqrt(nv)); nr = ceil(nv/nc);
else
    nr = o.Layout(1); nc = o.Layout(2);
end

useTiles = exist('tiledlayout','file')==2;
fig = [];

% ----------------------------------------------------------------------
% 3. Draw each figure
% ----------------------------------------------------------------------
for f = 1:numel(figLoop)
    sh = figLoop{f};

    fig = figure('Color','w','Units','centimeters');
    if isempty(o.FigSize)
        w = max(8, nc*5.0 + 1.2);
        h = max(6, nr*3.8 + 1.4 + (nSeries>1)*0.9);
        set(fig,'Position',[2 2 w h]);
    else
        set(fig,'Position',[2 2 o.FigSize(1) o.FigSize(2)]);
    end

    if useTiles
        tl = tiledlayout(fig, nr, nc, 'TileSpacing','compact','Padding','compact');
    end

    hLeg = gobjects(1, nSeries);   % line handles of the first panel, for the shared legend

    for i = 1:nv
        if useTiles, ax = nexttile(tl); else, ax = subplot(nr,nc,i,'Parent',fig); end
        hold(ax,'on');

        % --- Bands (one scenario, OverlayShocks off) ---
        if ~isempty(o.Bands) && nSeries==1 && ~o.OverlayShocks
            local_draw_bands(ax, o.Bands, vars{i}, sh, o.Scale, o.Horizon);
        end

        % --- Zero line ---
        if o.ZeroLine
            if exist('yline','file')==2
                yl = yline(ax, 0, '-'); yl.Color = [0.6 0.6 0.6]; yl.LineWidth = 0.5;
                yl.Annotation.LegendInformation.IconDisplayStyle = 'off';
            else
                plot(ax, [0 1e6], [0 0], '-', 'Color',[0.6 0.6 0.6], 'LineWidth',0.5, ...
                     'HandleVisibility','off');
            end
        end

        % --- One line per series ---
        for k = 1:nSeries
            y = seriesGet(k, sh, vars{i});
            H = o.Horizon; if isempty(H), H = numel(y); end
            if isempty(y), y = zeros(H,1); end
            H = min(H, numel(y));
            xx = (1:H)';
            col = o.Colors(mod(k-1,size(o.Colors,1))+1, :);
            lst = o.LineStyles{mod(k-1,numel(o.LineStyles))+1};
            hl = plot(ax, xx, o.Scale*y(1:H), 'LineStyle',lst, 'Color',col, ...
                      'LineWidth',o.LineWidth);
            if i==1, hLeg(k) = hl; end
        end

        % --- Panel style ---
        local_style_axis(ax, o);
        if isempty(o.Horizon)
            Hax = local_irflen(scen{1}, vars{i}, local_anyshock(sh, shocks));
        else
            Hax = o.Horizon;
        end
        xlim(ax, [1, max(2, Hax)]);

        % Title
        ttl = local_title(M_, vars{i}, o, i);
        [tstr, tint] = ttl{:};
        title(ax, tstr, 'Interpreter',tint, 'FontWeight','normal', ...
              'FontName',o.Font, 'FontSize',o.FontSize+1);

        % Axis labels: Y only on the left column, X only on the bottom row
        isLeftCol   = mod(i-1, nc)==0;
        isBottomRow = (i > nv-nc);
        if isLeftCol,   ylabel(ax, o.YLabel, 'FontName',o.Font,'FontSize',o.FontSize); end
        if isBottomRow, xlabel(ax, o.XLabel, 'FontName',o.Font,'FontSize',o.FontSize); end

        hold(ax,'off');
    end

    % --- Shared legend ---
    if nSeries > 1 && any(~cellfun(@isempty, seriesNm))
        if useTiles && exist('OCTAVE_VERSION','builtin')==0
            lgd = legend(hLeg, seriesNm, 'Orientation','horizontal', ...
                         'FontName',o.Font, 'FontSize',o.FontSize, 'Box','off');
            try, lgd.Layout.Tile = 'south'; catch, end
        else
            legend(hLeg, seriesNm, 'Box','off', 'FontName',o.Font, ...
                   'FontSize',o.FontSize, 'Location','best');
        end
    end

    % --- Shock title on top (several figures, one shock each) ---
    if ~o.OverlayShocks && ~isempty(sh) && numel(shocks) > 1
        shttl = local_shock_title(M_, sh);
        if useTiles
            title(tl, shttl, 'Interpreter','none', 'FontName',o.Font, ...
                  'FontSize',o.FontSize+2, 'FontWeight','bold');
        else
            sgtitle(shttl, 'FontName',o.Font, 'FontSize',o.FontSize+2);
        end
    end

    % --- Export ---
    if ~isempty(o.Save)
        base = o.Save;
        if ~o.OverlayShocks && numel(shocks) > 1, base = [base '_' sh]; end %#ok<AGROW>
        local_export(fig, base, o.Formats);
    end
end
end % ===== end of main function =====


% ======================================================================
% Local functions
% ======================================================================
function y = local_get(oo, var, shock)
% Return oo_.irfs.<var>_<shock>, or empty when Dynare did not store it: the variable is
% not in the stoch_simul list, or the shock is not in irf_shocks or has zero variance.
% irf_plot_threshold only hides Dynare's own plots; the field is still stored.
y = [];
if isempty(shock), return; end
fn = [var '_' shock];
if isfield(oo,'irfs') && isfield(oo.irfs, fn)
    y = oo.irfs.(fn)(:);
end
end

% ----------------------------------------------------------------------
function L = local_irflen(oo, var, shock)
% IRF length, for xlim.
y = local_get(oo, var, shock);
if isempty(y), L = 40; else, L = numel(y); end
end

function sh = local_anyshock(curShock, shocks)
if ~isempty(curShock), sh = curShock; elseif ~isempty(shocks), sh = shocks{1}; else, sh=''; end
end

% ----------------------------------------------------------------------
function [vars, shocks] = local_infer(oo, M_)
% Recover the sets of variables and shocks from the oo_.irfs field names and M_.exo_names.
vars = {}; shocks = {};
if ~isfield(oo,'irfs'), return; end
fns = fieldnames(oo.irfs);
exol = cellstr(M_.exo_names);
% Match the longest suffix first (shock names by descending length), so a short
% shock name does not match by mistake
[~, ord] = sort(cellfun(@numel, exol), 'descend');
exol = exol(ord);
for j = 1:numel(fns)
    fn = fns{j};
    for e = 1:numel(exol)
        suf = ['_' exol{e}];
        if numel(fn) > numel(suf) && strcmp(fn(end-numel(suf)+1:end), suf)
            vars{end+1}   = fn(1:end-numel(suf)); %#ok<AGROW>
            shocks{end+1} = exol{e};              %#ok<AGROW>
            break;
        end
    end
end
vars   = unique(vars,   'stable');
shocks = unique(shocks, 'stable');
end

% ----------------------------------------------------------------------
function out = local_title(M_, var, o, i)
% Return {string, interpreter}. Order: custom Titles, TeX name, long_name, variable name.
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
function s = local_shock_title(M_, shock)
% Figure title: the long_name of the shock; the shock name when long_name is missing
% or equal to the name.
s = shock;
names = cellstr(M_.exo_names);
idx = find(strcmp(names, shock), 1);
if ~isempty(idx) && isfield(M_,'exo_names_long')
    l = M_.exo_names_long;
    if iscell(l), cand = l{idx}; else, cand = strtrim(l(idx,:)); end
    if ~isempty(cand) && ~strcmp(cand, shock), s = cand; end
end
end

% ----------------------------------------------------------------------
function local_draw_bands(ax, Bands, var, shock, scale, H)
% Draw bands (grey shading). Bands is {lower,upper} or {{l1,u1},{l2,u2}} (wide band
% first, then narrow band).
if isempty(Bands), return; end
if ~iscell(Bands{1}), Bands = {Bands}; end   % make it a list of bands
greys = [0.78 0.78 0.78; 0.62 0.62 0.62];     % wide outer band light, narrow inner band dark
for b = 1:numel(Bands)
    seg = Bands{b};
    lo = local_band_series(seg{1}, var, shock);
    hi = local_band_series(seg{2}, var, shock);
    if isempty(lo) || isempty(hi), continue; end
    if isempty(H), H = numel(lo); end
    H = min([H numel(lo) numel(hi)]);
    xx = (1:H)';
    g = greys(min(b,size(greys,1)), :);
    pa = patch(ax, [xx; flipud(xx)], scale*[lo(1:H); flipud(hi(1:H))], g, ...
               'EdgeColor','none', 'FaceAlpha',0.55);
    pa.Annotation.LegendInformation.IconDisplayStyle = 'off';
end
end

function v = local_band_series(B, var, shock)
% Band data: a struct with fields <var>_<shock>, or a numeric array read as one
% vector (B(:)) in every panel.
v = [];
if isstruct(B)
    fn = [var '_' shock];
    if isfield(B, fn), v = B.(fn)(:); end
elseif isnumeric(B)
    v = B(:);
end
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
        fprintf('[plot_irfs_pub] Exported %s\n', fn);
    catch ME
        warning('plot_irfs_pub:export', 'Export of %s failed: %s', fn, ME.message);
    end
end
end

% ----------------------------------------------------------------------
function [vars, shocks, oo1, oo2, M] = local_demo_data()
% Synthetic RBC-style IRFs (two scenarios, one shock) for the demo without arguments.
H = 20; t = (0:H-1)';
vars = {'y','c','invest','l'}; shocks = {'eps_z'};
shape = @(a,b,p) a*exp(-t/p) .* (1 + b*sin(t/3));
mk = @(s,p0) struct('y_eps_z',      s*shape(1.0,0.0,6*p0), ...
                    'c_eps_z',      s*shape(0.5,0.0,9*p0), ...
                    'invest_eps_z', s*shape(3.0,0.2,4*p0), ...
                    'l_eps_z',      s*shape(0.4,0.1,5*p0));
oo1 = struct('irfs', mk(1.00, 1.0));    % baseline
oo2 = struct('irfs', mk(0.95, 0.6));    % lower persistence
M = struct();
M.endo_names      = {'y';'c';'invest';'l'};
M.endo_names_tex  = {'y';'c';'i';'\ell'};
M.endo_names_long = {'Output';'Consumption';'Investment';'Hours'};
M.exo_names       = {'eps_z'};
M.exo_names_long  = {'Technology shock'};
end
