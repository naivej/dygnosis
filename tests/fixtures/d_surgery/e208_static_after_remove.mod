// inventory: d_surgery_e208_static_after_remove
var c k;
varexo x;
parameters aa;
aa = 1;
model;
[static, name='e1'] c = aa*x*k;
[dynamic, name='e2'] c = aa*x*k(-1);
end;
shocks;
var x = 0.01;
end;
initval;
c = 1;
k = 1;
end;
model_remove('e2');