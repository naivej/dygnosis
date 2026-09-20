// inventory: quiet_prior_std_exo
// 7.1 accepts this file: a legal std/corr prior head
var R Pie Y;
varexo eps;
varexo_det dve;
parameters alpha;
alpha = 0.36;
model;
R = 0.5*R(-1) + eps;
Pie = R(-1);
Y = Pie(-1) + dve;
end;
initval;
R = 0;
Pie = 0;
Y = 0;
end;
shocks;
var eps; stderr 0.1;
end;
varobs Y Pie R;
std(eps).prior(shape=inv_gamma, mean=0.5, stdev=0.1);
