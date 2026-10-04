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

**Planejar cenas** faz o Claude dividir a narração em cenas pelas frases e escrever um prompt de imagem para cada uma, a partir do **modelo de prompts de imagem** ([Modelos](templates.md)) e das notas de estética do canal.

**Gerar N imagens** então desenha, numa só tarefa, cada cena que ainda não tem imagem. O Nano Banana desenha cada uma em 16:9 a 2K, e o Gemini cobra cada imagem; uma cena que o provedor recusa não para as outras. A estimativa sob o botão principal diz quanto a rodada custa. Planejar exige uma chave do Claude, e desenhar uma chave do Gemini ([Suas chaves de API](api-keys.md#providers)).

<a id="list"></a>
## As cenas

As cenas aparecem em ordem, cada uma com o tempo, a imagem e como está: **A gerar**, **Revisar**, **Falhou** ou pronta. Escolha uma cena para vê-la inteira; ↑ e ↓ percorrem as cenas, e Enter usa a nova imagem de uma cena em revisão.

<a id="scene"></a>
## Uma cena

A cena escolhida mostra o trecho da narração que ela cobre, a imagem e o **prompt de imagem**. **Editar prompt** muda o que o próximo desenho mostra (até 4.000 caracteres); uma cena cujo prompt você mudou ganha a marca **Editado**. **Detalhes da geração** diz qual modelo desenhou a imagem, com quantos tokens e qual versão do modelo de prompts.

<a id="redraw"></a>
## Desenhar de novo

**Desenhar de novo** faz uma nova imagem a partir do prompt da cena. A nova imagem espera ao lado da atual: **Usar a nova imagem** a substitui, **Manter a atual** descarta a nova. Até você decidir, a cena mostra **Revisar** e a etapa diz quantas esperam.

<a id="filter"></a>
## Todas ou pendentes

Acima das cenas, **Todas** mostra cada uma e **Pendentes** só as que têm algo por fazer: uma imagem para desenhar ou revisar (na etapa Clipes, um clipe para fazer ou revisar). Cada aba diz quantas cenas tem.

<a id="replan"></a>
## Planejar de novo

Quando a narração é refeita, os tempos das cenas não batem mais e a etapa mostra **Desatualizada**. **Planejar de novo** troca as cenas por novas. Se elas já têm imagens ou clipes, o Bardo pergunta antes, porque planejar de novo descarta esses arquivos.
