// inventory: w090_undecl
var y c log_y log_c;
varexo e u;
parameters rho betta alphag rho_ig;
rho = 0.9;
betta = 0.99;
alphag = 0.08;
rho_ig = 0.88;

model;
y = rho * y(-1) + e;
c = betta * c(+1) + u;
log_y = y;
log_c = c;
end;

shocks;
var e; stderr 0.01;
var u; stderr 0.01;
end;

varobs log_y log_c not_a_var;
estimated_params;
alphag, 0.08, 0.01, 0.15;
rho_ig, 0.88, 0.50, 0.995;
end;
