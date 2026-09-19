// inventory: d_surgery_e001_empty_tag_list
var c k;
varexo x;
parameters aa alph bet delt gam;
aa = 1;
alph = 0.33;
bet = 0.99;
delt = 0.025;
gam = 1;
model;
[name='e1'] c = -k + aa*x*k(-1)^alph + (1-delt)*k(-1);
[name='e2'] c^(-gam) = (aa*alph*x(+1)*k^(alph-1) + 1 - delt)*c(+1)^(-gam)/(1+bet);
end;
shocks;
var x = 0.01;
end;
model_remove();
