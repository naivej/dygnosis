// inventory: e020_alpph_typo
var y c;
varexo e;
parameters rho betta alppha;
rho = 0.9;
betta = 0.99;
alppha = 0.33;

model;
y = rho * y(-1) + alpph + e;
c = betta * c(+1);
end;

shocks;
var e; stderr 0.01;
end;

steady_state_model;
y = 0;
c = 0;
end;
stoch_simul;
