---
id: scenes
title: Cenas e imagens
group: production
place: projects/scenes
tour: scenes
---

# Cenas e imagens

As cenas dividem a narração nos planos que o vídeo mostra, cada um com uma imagem. Elas ficam na etapa [Cenas](bardo:go/projects/scenes) de um projeto, e a etapa [Clipes](clips.md) as anima. [Mostre a etapa para mim](bardo:tour/scenes).

<a id="plan"></a>
## Planejar e desenhar

**Planejar cenas** faz o Claude dividir a narração em cenas pelas frases e escrever um prompt de imagem e um [prompt de movimento](clips.md#motion) para cada uma, a partir do **modelo de prompts de imagem** ([Modelos](templates.md)) e das notas de estética do canal. Ele também lista as [pessoas que voltam](#cast) em mais de uma cena.

**Gerar N imagens** então desenha, numa só tarefa, cada cena que ainda não tem imagem. O Nano Banana desenha cada uma a 2K no formato do [vídeo](projects.md#format), 9:16 para um Short e 16:9 para um vídeo longo, e o Gemini cobra cada imagem; uma cena que o provedor recusa não para as outras. A estimativa sob o botão principal diz quanto a rodada custa. Planejar exige uma chave do Claude, e desenhar uma chave do Gemini ([Suas chaves de API](api-keys.md#providers)).

<a id="list"></a>
## As cenas

As cenas aparecem em ordem, cada uma com o tempo, a imagem e como está: **A gerar**, **Revisar**, **Falhou** ou pronta. Escolha uma cena para vê-la inteira; ↑ e ↓ percorrem as cenas, e Enter usa a nova imagem de uma cena em revisão.

<a id="scene"></a>
## Uma cena

A cena escolhida mostra o trecho da narração que ela cobre, a imagem e o **prompt de imagem**. **Editar prompt** muda o que o próximo desenho mostra (até 4.000 caracteres); uma cena cujo prompt você mudou ganha a marca **Editado**. **Detalhes da geração** diz qual modelo de IA desenhou a imagem, com quantos tokens e qual versão do modelo de prompts.

<a id="redraw"></a>
## Desenhar de novo

**Desenhar de novo** faz uma nova imagem a partir do prompt da cena. A nova imagem espera ao lado da atual: **Usar a nova imagem** a substitui, **Manter a atual** descarta a nova. Até você decidir, a cena mostra **Revisar** e a etapa diz quantas esperam.

<a id="cast"></a>
## Pessoas que voltam

Quando a mesma pessoa aparece em mais de uma cena (o chef, a detetive), o plano a lista uma vez com uma aparência fixa: sexo, idade, tom de pele, cabelo, porte e roupas. Antes de desenhar a primeira cena em que ela aparece, o Nano Banana desenha o **retrato de referência** dela, e toda cena com ela é desenhada com esse retrato e essa aparência, para que mantenha o mesmo rosto de cena para cena. O retrato é cobrado como uma imagem de cena, e a estimativa o conta.

Uma cena que mostra alguém que volta lista essa pessoa em **Pessoas nesta cena**, com o retrato. **Editar aparência** muda a descrição; o retrato deixa de corresponder a ela e sai, e a próxima cena desenhada com essa pessoa desenha um novo antes. **Desenhar retrato de novo** faz um novo retrato com a mesma aparência. Nos dois casos, as cenas já desenhadas mantêm a imagem: desenhe-as de novo para usar o novo retrato. Um retrato que o provedor recusa fica marcado na pessoa, e as cenas dela são desenhadas só pela descrição.

<a id="filter"></a>
## Todas ou pendentes

Acima das cenas, **Todas** mostra cada uma e **Pendentes** só as que têm algo por fazer: uma imagem para desenhar ou revisar (na etapa Clipes, um clipe para fazer ou revisar). Cada aba diz quantas cenas tem.

<a id="replan"></a>
## Planejar de novo

Quando a narração é refeita, os tempos das cenas não batem mais e a etapa mostra **Desatualizada**. **Planejar de novo** troca as cenas por novas. Se elas já têm imagens ou clipes, o Bardo pergunta antes, porque planejar de novo descarta esses arquivos.

<a id="place"></a>
## Posicionar as cenas numa versão

Uma [versão em outro idioma](script.md#versions) começa com as cenas do original, ainda nos tempos do original. Depois que a narração dela fica pronta (gerada ou importada), a etapa mostra **Cenas para posicionar** e oferece **Posicionar cenas nesta narração**. Cada cena mantém a imagem, o clipe e os prompts e vai para o novo tempo da frase em que começava; o texto dela passa a ser as palavras da nova narração. As frases são encontradas comparando as palavras da adaptação com o que foi narrado, então editar o roteiro antes não atrapalha.

Posicionar também monta o corte da versão a partir do original: o enquadramento, o movimento e a transição de cada cena, as cenas que ele deixou de fora, o estilo e o destaque das legendas, a mixagem e o formato do quadro. Música e SFX vão para o mesmo ponto da narração (a música mantém início e fim nas palavras, um SFX a sua duração). As legendas vêm da nova narração. Cortes e aparas dentro de uma cena, imagens importadas colocadas à mão e textos de legenda editados não vêm junto; refaça-os no [editor](editor.md).

As cenas esticam ou encolhem com as suas frases. **Vale conferir** lista as cenas que agora passam do clipe (o clipe segura o último quadro), as que cresceram mais da metade ou encolheram mais de um terço e as que a versão não narra, que ficam fora do corte. Depois de uma nova narração, posicionar volta a ser oferecido; **Planejar de novo** substitui as cenas como em qualquer projeto.
