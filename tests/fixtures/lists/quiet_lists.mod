// inventory: lists_quiet_lists
// One legal list on every command this slice walks, the masterplan
// `rplot y c e;` among them. 7.1 accepts the file.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

forecast y;
rplot y c e;
dynasave('f.csv') y c e;
dynatype('f.m') y c e;
shock_decomposition y;
realtime_shock_decomposition y;
initial_condition_decomposition y;
plot_shock_decomposition y c;
squeeze_shock_decomposition y;
stoch_simul y;
