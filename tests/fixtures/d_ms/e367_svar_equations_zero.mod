// inventory: e367_svar_equations_zero
// 7.1 refuses this file: `The value(s) passed to the 'equations' option must be greater than zero.`
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
svar(coefficients, chain=1, equations=[0, 1]);
