---
id: clips
title: Clipes
group: production
place: projects/clips
tour: clips
---

# Clipes

Um clipe anima a imagem de uma cena num vídeo curto. Os clipes são opcionais: uma cena sem clipe mostra a imagem parada no vídeo. Eles ficam na etapa [Clipes](bardo:go/projects/clips), que se abre quando uma cena tem imagem ([Cenas e imagens](scenes.md)). [Mostre a etapa para mim](bardo:tour/clips).

<a id="animate"></a>
## Animar as cenas

**Animar cenas sem clipe** envia ao provedor de vídeo, numa só tarefa, cada cena que tem imagem e não tem clipe; **Animar** numa cena faz só aquela. O [modo de produção](projects.md#mode) do vídeo diz quais cenas o primeiro botão envia: no **Gancho animado** ele vira **Animar o gancho** e envia só as cenas que começam nos primeiros 10 segundos, e no **Econômico** ele não aparece, já que todas as cenas ficam em imagens com movimento. **Animar** numa cena funciona em qualquer modo. Cada clipe dura o tempo em que a cena é narrada, dentro das durações que o modelo aceita. Uma cena que falha não para as outras, e tentar a tarefa de novo envia só as cenas ainda sem clipe.

Os clipes são feitos pela Higgsfield, pelo Kling ou pelo Google, com a sua própria chave de cada um ([Suas chaves de API](api-keys.md#providers)). Cancelar uma rodada para a espera, não o provedor: um clipe já enviado ainda pode ficar pronto e ser cobrado.

<a id="motion"></a>
## Movimento

O **prompt de movimento** diz como a imagem se move: um movimento de câmera e no máximo uma ação simples do assunto. O Claude escreve um para cada cena quando [planeja as cenas](scenes.md#plan); cenas planejadas antes disso usam o prompt de imagem, marcadas como **Igual ao prompt de imagem**. **Editar movimento** o muda para o próximo clipe da cena, e salvá-lo vazio volta ao do Claude.

<a id="model"></a>
## Modelo de vídeo

Cada cena usa o modelo padrão do vídeo, a menos que você escolha outro para ela em **Modelo de vídeo**: o do [modo de produção](projects.md#mode) (Kling 2.6 ou 3.0) ou, no **Tudo animado, modelo do canal**, o do canal, definido em [Canais](bardo:go/channels). Os modelos mudam no visual, nas durações que aceitam e no preço. Se o modelo de uma cena deixou de ser oferecido, a cena avisa; escolha outro.

<a id="review"></a>
## Revisar um clipe

Um novo clipe espera ao lado do atual como **Novo clipe para revisar**. **Reproduza-o** primeiro (**Reproduzir**), depois **Usar o novo clipe** ou **Descartar**. O clipe de uma cena mostra a duração e o modelo; **Usar a imagem parada** tira o clipe e volta à imagem. Quando a imagem da cena muda depois que o clipe foi feito, o clipe fica marcado como **Imagem mudou**.

<a id="cost"></a>
## Quanto custa um clipe

Sob o modelo, **Próximo clipe** diz quanto vai durar o próximo clipe da cena e quanto ele custa pela tarifa do provedor; a estimativa sob **Animar cenas sem clipe** soma a rodada. O provedor cobra cada segundo. Um modelo sem tarifa em [Custos](bardo:go/costs) avisa, e você pode cadastrar o preço dele lá.
