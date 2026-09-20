// inventory: e358_svar_identification_lag_twice
// 7.1 refuses this file: `lag 0 used more than once.`
var R Pie Y;
varobs Y Pie R;
parameters alpha;
alpha = 0.36;
model;
R = 0.5*R(-1);
Pie = alpha*R(-1);
Y = Pie(-1);
end;
initval;
R = 0;
Pie = 0;
Y = 0;
end;
shocks;
var R; stderr 0.1;
end;
svar_identification;
exclusion lag 0;
equation 1, Pie;
exclusion lag 0;
equation 2, Y;
end;
