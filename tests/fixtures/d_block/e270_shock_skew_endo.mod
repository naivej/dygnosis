// inventory: e270_shock_skew_endo
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
shocks;
skew y = 0;
end;
