// inventory: e238_stoch_simul_filters
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
stoch_simul(hp_filter=1600, bandpass_filter);
