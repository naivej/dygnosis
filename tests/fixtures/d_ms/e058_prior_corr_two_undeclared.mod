// inventory: e058_prior_corr_two_undeclared
// 7.1 refuses this file: `Unknown symbol: nosuch1.` — their run stops at the first name
var R Pie Y;
varexo eps;
parameters alpha;
alpha = 0.36;
model;
R = 0.5*R(-1) + eps;
Pie = R(-1);
Y = Pie(-1);
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
corr(nosuch1, nosuch2).prior(shape=beta, mean=0.5, stdev=0.1);
