// inventory: e363_svar_none_of_three
// 7.1 refuses this file: `You must pass one of 'coefficients', 'variances', or 'constants'.`
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
svar(chain=1);
